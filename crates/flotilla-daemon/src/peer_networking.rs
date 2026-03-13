use std::sync::Arc;

use flotilla_core::{config::ConfigStore, daemon::DaemonHandle, in_process::InProcessDaemon};
use flotilla_protocol::{
    ConfigLabel, DaemonEvent, HostName, PeerConnectionState, PeerDataMessage, PeerWireMessage, ProviderData, RepoIdentity,
    RoutedPeerMessage,
};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

use crate::peer::{InboundPeerEnvelope, OverlayUpdate, PeerManager, SshTransport};

/// Notification sent from connection sites to the outbound task when a
/// peer connects or reconnects. The outbound task responds by sending
/// current local state for all repos to the specific peer.
pub(crate) struct PeerConnectedNotice {
    pub(crate) peer: HostName,
    pub(crate) generation: u64,
}

/// Manages peer networking lifecycle: SSH connections, inbound message
/// processing, and outbound snapshot broadcasting.
///
/// Created via `new()` which loads peer config and sets up transports.
/// Call `spawn()` to start the three background task groups. The returned
/// `Arc<Mutex<PeerManager>>` and `mpsc::Sender<InboundPeerEnvelope>` let
/// `DaemonServer` feed inbound socket-peer messages into the same pipeline.
pub struct PeerNetworkingTask {
    daemon: Arc<InProcessDaemon>,
    peer_manager: Arc<Mutex<PeerManager>>,
    peer_data_tx: mpsc::Sender<InboundPeerEnvelope>,
    peer_data_rx: Option<mpsc::Receiver<InboundPeerEnvelope>>,
}

impl PeerNetworkingTask {
    /// Create a new peer networking task.
    ///
    /// Loads `hosts.toml` from config, creates a `PeerManager`, and registers
    /// SSH transports for each configured peer. Returns the task plus shared
    /// handles that `DaemonServer` needs for socket-peer integration.
    pub fn new(
        daemon: Arc<InProcessDaemon>,
        config: &ConfigStore,
    ) -> Result<(Self, Arc<Mutex<PeerManager>>, mpsc::Sender<InboundPeerEnvelope>), String> {
        let host_name = daemon.host_name().clone();
        let hosts_config = config.load_hosts()?;

        let peer_count = hosts_config.hosts.len();
        let mut peer_manager = PeerManager::new(host_name.clone());
        for (name, host_config) in hosts_config.hosts {
            let peer_host = HostName::new(&host_config.expected_host_name);
            if peer_host == host_name {
                warn!(
                    host = %host_name,
                    "peer config uses same name as local host — messages will be ignored"
                );
            }
            match SshTransport::new(host_name.clone(), ConfigLabel(name.clone()), host_config) {
                Ok(transport) => {
                    peer_manager.add_peer(peer_host, Box::new(transport));
                }
                Err(e) => {
                    warn!(host = %name, err = %e, "skipping peer with invalid host name");
                }
            }
        }

        info!(host = %host_name, %peer_count, "initialized PeerNetworkingTask");

        // Emit initial disconnected status for all configured peers
        for peer_host in peer_manager.configured_peer_names() {
            daemon.send_event(DaemonEvent::PeerStatusChanged { host: peer_host, status: PeerConnectionState::Disconnected });
        }

        let (peer_data_tx, peer_data_rx) = mpsc::channel(256);
        let peer_manager = Arc::new(Mutex::new(peer_manager));

        Ok((
            Self {
                daemon,
                peer_manager: Arc::clone(&peer_manager),
                peer_data_tx: peer_data_tx.clone(),
                peer_data_rx: Some(peer_data_rx),
            },
            peer_manager,
            peer_data_tx,
        ))
    }
}

/// Rebuild daemon overlays for repo identities affected by peer disconnect or failover.
pub(crate) async fn rebuild_peer_overlays(
    peer_manager: &Arc<Mutex<PeerManager>>,
    daemon: &Arc<InProcessDaemon>,
    affected_repos: Vec<RepoIdentity>,
) {
    for repo_id in affected_repos {
        if let Some(local_path) = daemon.find_repo_by_identity(&repo_id).await {
            // Local repo — rebuild its peer overlay from remaining peers
            let peers: Vec<(HostName, ProviderData)> = {
                let pm = peer_manager.lock().await;
                pm.get_peer_data()
                    .iter()
                    .filter_map(|(host, repos)| repos.get(&repo_id).map(|state| (host.clone(), state.provider_data.clone())))
                    .collect()
            };
            daemon.set_peer_providers(&local_path, peers).await;
        } else {
            // Remote-only repo — rebuild or remove depending on remaining peers
            let mut pm = peer_manager.lock().await;
            if pm.has_peer_data_for(&repo_id) {
                // Still has peer data — re-merge from remaining peers
                let peers: Vec<(HostName, ProviderData)> = pm
                    .get_peer_data()
                    .iter()
                    .filter_map(|(host, repos)| repos.get(&repo_id).map(|state| (host.clone(), state.provider_data.clone())))
                    .collect();

                if let Some(synthetic_path) = pm.known_remote_repos().get(&repo_id).cloned() {
                    drop(pm);
                    daemon.set_peer_providers(&synthetic_path, peers).await;
                }
            } else if let Some(synthetic_path) = pm.unregister_remote_repo(&repo_id) {
                // No peers remain — remove the virtual tab
                drop(pm);
                info!(
                    repo = %repo_id,
                    path = %synthetic_path.display(),
                    "removing virtual repo — no peers remaining"
                );
                if let Err(e) = daemon.remove_repo(&synthetic_path).await {
                    warn!(
                        repo = %repo_id,
                        err = %e,
                        "failed to remove virtual repo"
                    );
                }
            }
        }
    }
}

pub(crate) async fn dispatch_resync_requests(peer_manager: &Arc<Mutex<PeerManager>>, requests: Vec<RoutedPeerMessage>) {
    for request in requests {
        let target = match &request {
            RoutedPeerMessage::RequestResync { target_host, .. } => target_host.clone(),
            RoutedPeerMessage::ResyncSnapshot { requester_host, .. } => requester_host.clone(),
        };
        let sender = {
            let pm = peer_manager.lock().await;
            pm.resolve_sender(&target)
        };
        let sender = match sender {
            Ok(sender) => sender,
            Err(e) => {
                warn!(peer = %target, err = %e, "failed to resolve routed resync sender");
                continue;
            }
        };
        if let Err(e) = sender.send(PeerWireMessage::Routed(request)).await {
            warn!(peer = %target, err = %e, "failed to dispatch routed resync request");
        }
    }
}

pub(crate) async fn disconnect_peer_and_rebuild(
    peer_manager: &Arc<Mutex<PeerManager>>,
    daemon: &Arc<InProcessDaemon>,
    peer_name: &HostName,
    generation: u64,
) -> crate::peer::DisconnectPlan {
    let mut plan = {
        let mut pm = peer_manager.lock().await;
        pm.disconnect_peer(peer_name, generation)
    };

    // Apply pre-computed overlay updates outside the PeerManager lock.
    // Identity → path is resolved here at apply time (not at computation time)
    // to avoid TOCTOU with concurrent add_repo/remove_repo.
    //
    // Note: a residual apply-ordering race exists here. Between releasing
    // the PM lock above and calling set_peer_providers below, the central
    // processor can accept fresh inbound data and call set_peer_providers
    // via the HandleResult::Updated path. Because set_peer_providers is a
    // blind replace, this apply could overwrite that newer data. This is
    // the same read-then-apply pattern shared by ALL overlay write paths
    // (including the central processor itself). The effect is transient:
    // the next inbound message for the affected repo will re-apply the
    // correct state. Fully fixing this requires versioned/conditional
    // set_peer_providers, which is a broader change tracked separately.
    for update in &plan.overlay_updates {
        match update {
            OverlayUpdate::SetProviders { identity, peers } => {
                // Resolve identity to current local path. For remote-only repos,
                // the path comes from known_remote_repos (already resolved in the plan).
                // For local repos that were removed concurrently, find_repo_by_identity
                // returns None and we skip — the repo is gone, no overlay needed.
                if let Some(local_path) = daemon.find_repo_by_identity(identity).await {
                    daemon.set_peer_providers(&local_path, peers.clone()).await;
                } else if let Some(synthetic_path) = {
                    let pm = peer_manager.lock().await;
                    pm.known_remote_repos().get(identity).cloned()
                } {
                    daemon.set_peer_providers(&synthetic_path, peers.clone()).await;
                }
            }
            OverlayUpdate::RemoveRepo { identity, path } => {
                info!(
                    repo = %identity,
                    path = %path.display(),
                    "removing virtual repo — no peers remaining"
                );
                if let Err(e) = daemon.remove_repo(path).await {
                    warn!(
                        repo = %identity,
                        err = %e,
                        "failed to remove virtual repo"
                    );
                }
            }
        }
    }

    let resync_requests = std::mem::take(&mut plan.resync_requests);
    dispatch_resync_requests(peer_manager, resync_requests).await;
    plan
}

/// Send local-only provider data to all peers for a given repo.
///
/// Called by the outbound task whenever any snapshot event (full or delta)
/// indicates local data changed. Always sends a full snapshot to peers —
/// peer replication doesn't use deltas.
///
/// Sends to both configured SSH transports (outbound peers we connected to)
/// and inbound peer clients (peers that connected to our socket).
/// Returns `true` if at least one peer was successfully sent to.
pub(crate) async fn send_local_to_peers(
    daemon: &Arc<InProcessDaemon>,
    peer_manager: &Arc<Mutex<PeerManager>>,
    host_name: &HostName,
    clock: &mut flotilla_protocol::VectorClock,
    repo_path: &std::path::Path,
    local_providers: ProviderData,
    local_data_version: u64,
) -> bool {
    let Some(identity) = daemon.find_identity_for_path(repo_path).await else {
        return false;
    };

    clock.tick(host_name);
    let msg = PeerDataMessage {
        origin_host: host_name.clone(),
        repo_identity: identity,
        repo_path: repo_path.to_path_buf(),
        clock: clock.clone(),
        kind: flotilla_protocol::PeerDataKind::Snapshot { data: Box::new(local_providers), seq: local_data_version },
    };

    // Send to all active peers, including direct socket peers.
    let peer_senders = {
        let pm = peer_manager.lock().await;
        pm.active_peer_senders()
    };
    let mut any_sent = false;
    for (peer_name, sender) in peer_senders {
        if let Err(e) = sender.send(PeerWireMessage::Data(msg.clone())).await {
            debug!(peer = %peer_name, err = %e, "failed to send snapshot to peer");
        } else {
            any_sent = true;
        }
    }
    any_sent
}

/// Send current local state for all repos to a specific newly-connected peer.
///
/// Unlike `send_local_to_peers` (which broadcasts), this targets a single peer
/// that has just connected and has no state. Bypasses `last_sent_versions` since
/// the peer needs everything regardless of what was previously sent to others.
///
/// The generation guard ensures this is a no-op if the connection has already
/// been superseded between the notice being sent and this function running.
pub(crate) async fn send_local_to_peer(
    daemon: &Arc<InProcessDaemon>,
    peer_manager: &Arc<Mutex<PeerManager>>,
    host_name: &HostName,
    clock: &mut flotilla_protocol::VectorClock,
    peer: &HostName,
    generation: u64,
) -> bool {
    let repo_paths = daemon.tracked_repo_paths().await;
    let mut any_sent = false;

    // Resolve the sender once before iterating repos. If the connection
    // has already been superseded, skip the entire loop.
    let sender = {
        let pm = peer_manager.lock().await;
        pm.get_sender_if_current(peer, generation)
    };
    let Some(sender) = sender else {
        debug!(peer = %peer, "peer connection superseded, skipping local state send");
        return false;
    };

    for repo_path in repo_paths {
        let Some((local_providers, version)) = daemon.get_local_providers(&repo_path).await else {
            continue;
        };
        // Skip uninitialized repos (version 0 = not yet refreshed). Sending
        // empty data with a fresh vector clock would overwrite the peer's
        // existing state. The peer will receive data on first local refresh.
        if version == 0 {
            continue;
        }
        let Some(identity) = daemon.find_identity_for_path(&repo_path).await else {
            continue;
        };

        clock.tick(host_name);
        let msg = PeerDataMessage {
            origin_host: host_name.clone(),
            repo_identity: identity,
            repo_path: repo_path.clone(),
            clock: clock.clone(),
            kind: flotilla_protocol::PeerDataKind::Snapshot { data: Box::new(local_providers), seq: version },
        };

        if let Err(e) = sender.send(PeerWireMessage::Data(msg)).await {
            debug!(peer = %peer, err = %e, "failed to send local state to peer");
        } else {
            any_sent = true;
        }
    }
    any_sent
}

/// Forward messages from an inbound receiver to the shared peer_data channel.
///
/// Returns `true` if the inbound receiver was closed (connection dropped),
/// `false` if the outbound channel was closed (daemon shutting down).
pub(crate) async fn forward_until_closed(
    tx: &mpsc::Sender<InboundPeerEnvelope>,
    inbound_rx: &mut mpsc::Receiver<PeerWireMessage>,
    peer_name: &HostName,
    generation: u64,
) -> bool {
    while let Some(msg) = inbound_rx.recv().await {
        if let Err(e) = tx.send(InboundPeerEnvelope { msg, connection_generation: generation, connection_peer: peer_name.clone() }).await {
            warn!(peer = %peer_name, err = %e, "forwarding channel closed");
            return false;
        }
    }
    true
}
