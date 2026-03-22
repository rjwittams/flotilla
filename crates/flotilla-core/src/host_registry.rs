use std::collections::{hash_map::Entry, HashMap, HashSet};

use flotilla_protocol::{
    DaemonEvent, HostListEntry, HostListResponse, HostProvidersResponse, HostSnapshot, HostStatusResponse, HostSummary,
    PeerConnectionState, StreamKey, SystemInfo, ToolInventory, TopologyResponse, TopologyRoute,
};
use tokio::sync::RwLock;

use crate::HostName;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HostCounts {
    pub(crate) repo_count: usize,
    pub(crate) work_item_count: usize,
}

#[derive(Debug, Clone)]
struct HostState {
    connection_status: PeerConnectionState,
    summary: Option<HostSummary>,
    seq: u64,
    removed: bool,
}

// ---------------------------------------------------------------------------
// HostRegistry
// ---------------------------------------------------------------------------

pub(crate) struct HostRegistry {
    host_name: HostName,
    hosts: RwLock<HashMap<HostName, HostState>>,
    configured_peer_names: RwLock<HashSet<HostName>>,
    topology_routes: RwLock<Vec<TopologyRoute>>,
    local_host_summary: HostSummary,
}

impl HostRegistry {
    pub(crate) fn new(host_name: HostName, local_host_summary: HostSummary) -> Self {
        let mut hosts = HashMap::new();
        hosts.insert(host_name.clone(), HostState {
            connection_status: PeerConnectionState::Connected,
            summary: Some(local_host_summary.clone()),
            seq: 1,
            removed: false,
        });
        Self {
            host_name,
            hosts: RwLock::new(hosts),
            configured_peer_names: RwLock::new(HashSet::new()),
            topology_routes: RwLock::new(Vec::new()),
            local_host_summary,
        }
    }

    pub(crate) fn host_name(&self) -> &HostName {
        &self.host_name
    }

    pub(crate) fn local_host_summary(&self) -> &HostSummary {
        &self.local_host_summary
    }

    // -----------------------------------------------------------------------
    // Query methods
    // -----------------------------------------------------------------------

    /// Returns the current connection status for a peer host.
    pub(crate) async fn peer_connection_status(&self, host: &HostName) -> PeerConnectionState {
        self.hosts
            .read()
            .await
            .get(host)
            .filter(|state| !state.removed)
            .map(|state| state.connection_status.clone())
            .unwrap_or(PeerConnectionState::Disconnected)
    }

    pub(crate) async fn list_hosts(&self, local_counts: HostCounts, remote_counts: &HashMap<HostName, HostCounts>) -> HostListResponse {
        let configured = self.configured_peer_names.read().await.clone();
        let (statuses, summaries) = self.active_host_maps().await;

        let hosts = known_hosts(&self.host_name, &configured, &statuses, &summaries, remote_counts)
            .into_iter()
            .map(|host| build_host_list_entry(&host, &self.host_name, &configured, &statuses, &summaries, local_counts, remote_counts))
            .collect();

        HostListResponse { hosts }
    }

    pub(crate) async fn get_host_status(
        &self,
        host: &str,
        local_counts: HostCounts,
        remote_counts: &HashMap<HostName, HostCounts>,
    ) -> Result<HostStatusResponse, String> {
        let configured = self.configured_peer_names.read().await.clone();
        let (statuses, summaries) = self.active_host_maps().await;
        let known = known_hosts(&self.host_name, &configured, &statuses, &summaries, remote_counts);
        let resolved = known.into_iter().find(|candidate| candidate.as_str() == host).ok_or_else(|| format!("host not found: {host}"))?;
        let summary = if resolved == self.host_name { Some(self.local_host_summary.clone()) } else { summaries.get(&resolved).cloned() };

        Ok(build_host_status(&resolved, &self.host_name, &configured, &statuses, summary, local_counts, remote_counts))
    }

    pub(crate) async fn get_host_providers(
        &self,
        host: &str,
        remote_counts: &HashMap<HostName, HostCounts>,
    ) -> Result<HostProvidersResponse, String> {
        let configured = self.configured_peer_names.read().await.clone();
        let (statuses, summaries) = self.active_host_maps().await;
        let known = known_hosts(&self.host_name, &configured, &statuses, &summaries, remote_counts);
        let resolved = known.into_iter().find(|candidate| candidate.as_str() == host).ok_or_else(|| format!("host not found: {host}"))?;
        let summary = if resolved == self.host_name {
            self.local_host_summary.clone()
        } else {
            summaries.get(&resolved).cloned().ok_or_else(|| format!("no summary available for host: {host}"))?
        };

        Ok(build_host_providers(&resolved, &self.host_name, &configured, &statuses, summary))
    }

    pub(crate) async fn get_topology(&self) -> TopologyResponse {
        let routes = self.topology_routes.read().await.clone();
        let configured = self.configured_peer_names.read().await.clone();
        build_topology(&self.host_name, &routes, &configured)
    }

    pub(crate) async fn replay_host_events(&self, last_seen: &HashMap<StreamKey, u64>) -> Vec<DaemonEvent> {
        let mut events = Vec::new();
        for (host_name, state) in self.hosts.read().await.iter() {
            let stream_key = StreamKey::Host { host_name: host_name.clone() };
            let up_to_date = last_seen.get(&stream_key).is_some_and(|seq| *seq == state.seq);
            if up_to_date {
                continue;
            }
            if state.removed {
                if last_seen.contains_key(&stream_key) {
                    events.push(DaemonEvent::HostRemoved { host: host_name.clone(), seq: state.seq });
                }
            } else {
                events.push(DaemonEvent::HostSnapshot(Box::new(build_host_snapshot(&self.host_name, host_name, state))));
            }
        }
        events
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Extracts connection-status and summary maps from `hosts`, filtering out removed entries.
    async fn active_host_maps(&self) -> (HashMap<HostName, PeerConnectionState>, HashMap<HostName, HostSummary>) {
        let hosts = self.hosts.read().await;
        let active: HashMap<_, _> = hosts.iter().filter(|(_, state)| !state.removed).map(|(h, s)| (h.clone(), s.clone())).collect();
        let statuses = active.iter().map(|(host, state)| (host.clone(), state.connection_status.clone())).collect();
        let summaries = active.iter().filter_map(|(host, state)| state.summary.clone().map(|summary| (host.clone(), summary))).collect();
        (statuses, summaries)
    }

    // -----------------------------------------------------------------------
    // Mutation methods
    // -----------------------------------------------------------------------

    /// Reconcile host membership: ensure configured peers and hosts with
    /// remote repo counts are present; remove hosts that no longer qualify.
    ///
    /// Takes `remote_counts` as a parameter (the caller owns the repo data)
    /// and emits events via the `emit` closure.
    async fn sync_host_membership(&self, remote_counts: &HashMap<HostName, HostCounts>, emit: &impl Fn(DaemonEvent)) {
        let configured = self.configured_peer_names.read().await.clone();
        let mut hosts = self.hosts.write().await;

        for host_name in configured.iter().chain(remote_counts.keys()) {
            if host_name != &self.host_name {
                match hosts.entry(host_name.clone()) {
                    Entry::Vacant(entry) => {
                        let state = entry.insert(HostState {
                            connection_status: PeerConnectionState::Disconnected,
                            summary: None,
                            seq: 1,
                            removed: false,
                        });
                        emit(DaemonEvent::HostSnapshot(Box::new(build_host_snapshot(&self.host_name, host_name, state))));
                    }
                    Entry::Occupied(mut entry) => {
                        let state = entry.get_mut();
                        if state.removed {
                            state.removed = false;
                            state.seq += 1;
                            emit(DaemonEvent::HostSnapshot(Box::new(build_host_snapshot(&self.host_name, host_name, state))));
                        }
                    }
                }
            }
        }

        let host_names: Vec<_> = hosts.keys().cloned().collect();
        for host_name in host_names {
            let Some(state) = hosts.get(&host_name) else {
                continue;
            };
            if should_present_host_state(&self.host_name, &configured, remote_counts, &host_name, state) {
                continue;
            }
            if let Some(seq) = mark_host_removed(&mut hosts, &host_name) {
                emit(DaemonEvent::HostRemoved { host: host_name, seq });
            }
        }
    }

    /// Publish a peer connection status change, emitting `PeerStatusChanged`
    /// and `HostSnapshot` events, then reconciling membership.
    pub(crate) async fn publish_peer_connection_status(
        &self,
        host: &HostName,
        status: PeerConnectionState,
        remote_counts: &HashMap<HostName, HostCounts>,
        emit: &impl Fn(DaemonEvent),
    ) -> Option<HostSnapshot> {
        let snapshot = {
            let mut hosts = self.hosts.write().await;
            update_host_status(&self.host_name, &mut hosts, host, status.clone())
        };
        if let Some(snapshot) = snapshot.as_ref() {
            emit(DaemonEvent::PeerStatusChanged { host: host.clone(), status });
            emit(DaemonEvent::HostSnapshot(Box::new(snapshot.clone())));
        }
        self.sync_host_membership(remote_counts, emit).await;
        snapshot
    }

    /// Publish a peer host summary update. Normalizes the `host_name` field
    /// and emits a `HostSnapshot` if the summary changed. Does NOT call
    /// `sync_host_membership` (matches current behavior).
    pub(crate) async fn publish_peer_summary(
        &self,
        host: &HostName,
        summary: HostSummary,
        emit: &impl Fn(DaemonEvent),
    ) -> Option<HostSnapshot> {
        let mut summary = summary;
        summary.host_name = host.clone();
        let snapshot = {
            let mut hosts = self.hosts.write().await;
            update_host_summary(&self.host_name, &mut hosts, host, summary)
        };
        if let Some(snapshot) = snapshot.as_ref() {
            emit(DaemonEvent::HostSnapshot(Box::new(snapshot.clone())));
        }
        snapshot
    }

    /// Update the set of configured peer names, then reconcile membership.
    pub(crate) async fn set_configured_peer_names(
        &self,
        peers: Vec<HostName>,
        remote_counts: &HashMap<HostName, HostCounts>,
        emit: &impl Fn(DaemonEvent),
    ) {
        let mut configured = self.configured_peer_names.write().await;
        *configured = peers.iter().cloned().collect();
        drop(configured);

        self.sync_host_membership(remote_counts, emit).await;
    }

    /// Replace the peer host summaries map. Clears summaries for peers no
    /// longer present, updates/adds summaries for peers in the new map, then
    /// reconciles membership.
    pub(crate) async fn set_peer_host_summaries(
        &self,
        summaries: HashMap<HostName, HostSummary>,
        remote_counts: &HashMap<HostName, HostCounts>,
        emit: &impl Fn(DaemonEvent),
    ) {
        let mut normalized = HashMap::new();
        for (host_name, mut summary) in summaries {
            summary.host_name = host_name.clone();
            normalized.insert(host_name, summary);
        }

        {
            let mut hosts = self.hosts.write().await;
            let host_names: Vec<_> = hosts.keys().cloned().collect();
            for host_name in host_names {
                if !normalized.contains_key(&host_name) {
                    if let Some(snapshot) = clear_host_summary(&self.host_name, &mut hosts, &host_name) {
                        emit(DaemonEvent::HostSnapshot(Box::new(snapshot)));
                    }
                }
            }
            for (host_name, summary) in normalized {
                if let Some(snapshot) = update_host_summary(&self.host_name, &mut hosts, &host_name, summary) {
                    emit(DaemonEvent::HostSnapshot(Box::new(snapshot)));
                }
            }
        }

        self.sync_host_membership(remote_counts, emit).await;
    }

    /// Replace the topology routes. Sorts defensively for stable output.
    pub(crate) async fn set_topology_routes(&self, mut routes: Vec<TopologyRoute>) {
        routes.sort_by(|a, b| a.target.cmp(&b.target));
        let mut stored = self.topology_routes.write().await;
        *stored = routes;
    }

    /// Mirror host-related events into internal state. Best-effort: if the
    /// lock is contended the update is silently skipped. Must be synchronous
    /// (`fn`, not `async fn`) because it is called from synchronous contexts.
    pub(crate) fn apply_event(&self, event: &DaemonEvent) {
        match event {
            DaemonEvent::PeerStatusChanged { host, status } => {
                if let Ok(mut hosts) = self.hosts.try_write() {
                    let _ = update_host_status(&self.host_name, &mut hosts, host, status.clone());
                }
            }
            DaemonEvent::HostSnapshot(snap) => {
                if let Ok(mut hosts) = self.hosts.try_write() {
                    let mut summary = snap.summary.clone();
                    summary.host_name = snap.host_name.clone();
                    match hosts.get_mut(&snap.host_name) {
                        Some(state) if state.seq == snap.seq => {
                            state.connection_status = snap.connection_status.clone();
                            state.removed = false;
                        }
                        Some(state) if state.seq < snap.seq => {
                            *state = HostState {
                                connection_status: snap.connection_status.clone(),
                                summary: Some(summary),
                                seq: snap.seq,
                                removed: false,
                            };
                        }
                        None => {
                            hosts.insert(snap.host_name.clone(), HostState {
                                connection_status: snap.connection_status.clone(),
                                summary: Some(summary),
                                seq: snap.seq,
                                removed: false,
                            });
                        }
                        _ => {}
                    }
                }
            }
            DaemonEvent::HostRemoved { host, seq } => {
                if let Ok(mut hosts) = self.hosts.try_write() {
                    if let Some(state) = hosts.get_mut(host) {
                        if state.seq <= *seq {
                            state.connection_status = PeerConnectionState::Disconnected;
                            state.summary = None;
                            state.seq = *seq;
                            state.removed = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions (from in_process.rs)
// ---------------------------------------------------------------------------

fn default_host_summary(host_name: &HostName) -> HostSummary {
    HostSummary { host_name: host_name.clone(), system: SystemInfo::default(), inventory: ToolInventory::default(), providers: vec![] }
}

fn ensure_remote_host_state<'a>(hosts: &'a mut HashMap<HostName, HostState>, host_name: &HostName) -> &'a mut HostState {
    hosts.entry(host_name.clone()).or_insert_with(|| HostState {
        connection_status: PeerConnectionState::Disconnected,
        summary: None,
        seq: 1,
        removed: false,
    })
}

fn build_host_snapshot(local_host: &HostName, host_name: &HostName, state: &HostState) -> HostSnapshot {
    debug_assert!(!state.removed, "removed hosts should not be materialized as snapshots");
    HostSnapshot {
        seq: state.seq,
        host_name: host_name.clone(),
        is_local: *host_name == *local_host,
        connection_status: state.connection_status.clone(),
        summary: state.summary.clone().unwrap_or_else(|| default_host_summary(host_name)),
    }
}

fn update_host_status(
    local_host: &HostName,
    hosts: &mut HashMap<HostName, HostState>,
    host_name: &HostName,
    status: PeerConnectionState,
) -> Option<HostSnapshot> {
    let state = ensure_remote_host_state(hosts, host_name);
    if !state.removed && state.connection_status == status {
        return None;
    }
    state.connection_status = status;
    state.removed = false;
    state.seq += 1;
    Some(build_host_snapshot(local_host, host_name, state))
}

fn update_host_summary(
    local_host: &HostName,
    hosts: &mut HashMap<HostName, HostState>,
    host_name: &HostName,
    summary: HostSummary,
) -> Option<HostSnapshot> {
    let state = ensure_remote_host_state(hosts, host_name);
    if !state.removed && state.summary.as_ref() == Some(&summary) {
        return None;
    }
    state.summary = Some(summary);
    state.removed = false;
    state.seq += 1;
    Some(build_host_snapshot(local_host, host_name, state))
}

fn clear_host_summary(local_host: &HostName, hosts: &mut HashMap<HostName, HostState>, host_name: &HostName) -> Option<HostSnapshot> {
    if host_name == local_host {
        return None;
    }
    let state = hosts.get_mut(host_name)?;
    state.summary.as_ref()?;
    state.summary = None;
    state.removed = false;
    state.seq += 1;
    Some(build_host_snapshot(local_host, host_name, state))
}

fn should_present_host_state(
    local_host: &HostName,
    configured: &HashSet<HostName>,
    remote_counts: &HashMap<HostName, HostCounts>,
    host_name: &HostName,
    state: &HostState,
) -> bool {
    host_name == local_host
        || configured.contains(host_name)
        || state.connection_status != PeerConnectionState::Disconnected
        || state.summary.is_some()
        || remote_counts.contains_key(host_name)
}

fn mark_host_removed(hosts: &mut HashMap<HostName, HostState>, host_name: &HostName) -> Option<u64> {
    let state = hosts.get_mut(host_name)?;
    if state.removed {
        return None;
    }
    state.connection_status = PeerConnectionState::Disconnected;
    state.summary = None;
    state.removed = true;
    state.seq += 1;
    Some(state.seq)
}

// ---------------------------------------------------------------------------
// Query functions (from host_queries.rs)
// ---------------------------------------------------------------------------

fn known_hosts(
    local_host: &HostName,
    configured: &HashSet<HostName>,
    statuses: &HashMap<HostName, PeerConnectionState>,
    summaries: &HashMap<HostName, HostSummary>,
    remote_counts: &HashMap<HostName, HostCounts>,
) -> Vec<HostName> {
    let mut hosts = HashSet::from([local_host.clone()]);
    hosts.extend(configured.iter().cloned());
    hosts.extend(statuses.keys().cloned());
    hosts.extend(summaries.keys().cloned());
    hosts.extend(remote_counts.keys().cloned());

    let mut hosts: Vec<_> = hosts.into_iter().collect();
    hosts.sort_by(|a, b| {
        let a_local = a == local_host;
        let b_local = b == local_host;
        b_local.cmp(&a_local).then_with(|| a.cmp(b))
    });
    hosts
}

fn connection_status(host: &HostName, local_host: &HostName, statuses: &HashMap<HostName, PeerConnectionState>) -> PeerConnectionState {
    if host == local_host {
        PeerConnectionState::Connected
    } else {
        statuses.get(host).cloned().unwrap_or(PeerConnectionState::Disconnected)
    }
}

fn build_host_list_entry(
    host: &HostName,
    local_host: &HostName,
    configured: &HashSet<HostName>,
    statuses: &HashMap<HostName, PeerConnectionState>,
    summaries: &HashMap<HostName, HostSummary>,
    local_counts: HostCounts,
    remote_counts: &HashMap<HostName, HostCounts>,
) -> HostListEntry {
    let is_local = host == local_host;
    let counts = if is_local { local_counts } else { remote_counts.get(host).copied().unwrap_or_default() };

    HostListEntry {
        host: host.clone(),
        is_local,
        configured: !is_local && configured.contains(host),
        connection_status: connection_status(host, local_host, statuses),
        has_summary: is_local || summaries.contains_key(host),
        repo_count: counts.repo_count,
        work_item_count: counts.work_item_count,
    }
}

fn build_host_status(
    host: &HostName,
    local_host: &HostName,
    configured: &HashSet<HostName>,
    statuses: &HashMap<HostName, PeerConnectionState>,
    summary: Option<HostSummary>,
    local_counts: HostCounts,
    remote_counts: &HashMap<HostName, HostCounts>,
) -> HostStatusResponse {
    let is_local = host == local_host;
    let counts = if is_local { local_counts } else { remote_counts.get(host).copied().unwrap_or_default() };

    HostStatusResponse {
        host: host.clone(),
        is_local,
        configured: !is_local && configured.contains(host),
        connection_status: connection_status(host, local_host, statuses),
        summary,
        repo_count: counts.repo_count,
        work_item_count: counts.work_item_count,
    }
}

fn build_host_providers(
    host: &HostName,
    local_host: &HostName,
    configured: &HashSet<HostName>,
    statuses: &HashMap<HostName, PeerConnectionState>,
    summary: HostSummary,
) -> HostProvidersResponse {
    HostProvidersResponse {
        host: host.clone(),
        is_local: host == local_host,
        configured: host != local_host && configured.contains(host),
        connection_status: connection_status(host, local_host, statuses),
        summary,
    }
}

fn build_topology(local_host: &HostName, routes: &[TopologyRoute], configured_peers: &HashSet<HostName>) -> TopologyResponse {
    let mut all_routes = routes.to_vec();

    // Include configured peers that have no routes (never connected).
    // `direct: true` is a placeholder — no relay is known yet.
    // Clients should treat `direct` as meaningless when `connected` is false.
    for peer in configured_peers {
        if peer == local_host {
            continue;
        }
        if !routes.iter().any(|r| r.target == *peer) {
            all_routes.push(TopologyRoute {
                target: peer.clone(),
                next_hop: peer.clone(),
                direct: true,
                connected: false,
                fallbacks: vec![],
            });
        }
    }

    all_routes.sort_by(|a, b| a.target.cmp(&b.target));
    TopologyResponse { local_host: local_host.clone(), routes: all_routes }
}
