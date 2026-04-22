// Watches Convoy + Presentation resources and emits namespace-scoped
// snapshots and deltas for the TUI. Single-writer for the namespace
// stream seq counter.
//
// Spec: docs/superpowers/specs/2026-04-21-tui-convoy-view-design.md §Architecture.

use std::collections::HashMap;

use flotilla_protocol::{
    namespace::{ConvoyId, ConvoySummary},
    DaemonEvent,
};
use tokio::sync::mpsc;

/// In-memory view of one namespace's convoys, owned by the projection.
#[allow(dead_code)]
#[derive(Default)]
struct NamespaceView {
    convoys: HashMap<ConvoyId, ConvoySummary>,
    seq: u64,
}

#[allow(dead_code)]
pub struct ConvoyProjection {
    namespaces: HashMap<String, NamespaceView>,
    /// Emitter for events going to connected clients.
    event_tx: mpsc::Sender<DaemonEvent>,
}

impl ConvoyProjection {
    pub fn new(event_tx: mpsc::Sender<DaemonEvent>) -> Self {
        Self { namespaces: HashMap::new(), event_tx }
    }
}
