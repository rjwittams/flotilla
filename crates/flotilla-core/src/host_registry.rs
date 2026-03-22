use std::collections::{HashMap, HashSet};

use flotilla_protocol::{
    HostListEntry, HostProvidersResponse, HostSnapshot, HostStatusResponse, HostSummary, PeerConnectionState, SystemInfo, ToolInventory,
    TopologyResponse, TopologyRoute,
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
