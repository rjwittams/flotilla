// Watches Convoy + Presentation resources and emits namespace-scoped
// snapshots and deltas for the TUI. Single-writer for the namespace
// stream seq counter.
//
// Spec: docs/superpowers/specs/2026-04-21-tui-convoy-view-design.md §Architecture.

use std::collections::HashMap;

use flotilla_protocol::{
    namespace::{
        ConvoyId, ConvoyPhase as WireConvoyPhase, ConvoySummary, ProcessSummary, TaskPhase as WireTaskPhase, TaskSummary,
    },
    DaemonEvent,
};
use flotilla_resources::{
    ConvoyPhase as ResConvoyPhase, ProcessSource, ResourceObject, SnapshotTask, TaskPhase as ResTaskPhase, TaskState,
    Convoy,
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

#[allow(dead_code)]
fn wire_convoy_phase(phase: ResConvoyPhase) -> WireConvoyPhase {
    match phase {
        ResConvoyPhase::Pending => WireConvoyPhase::Pending,
        ResConvoyPhase::Active => WireConvoyPhase::Active,
        ResConvoyPhase::Completed => WireConvoyPhase::Completed,
        ResConvoyPhase::Failed => WireConvoyPhase::Failed,
        ResConvoyPhase::Cancelled => WireConvoyPhase::Cancelled,
    }
}

#[allow(dead_code)]
fn wire_task_phase(phase: ResTaskPhase) -> WireTaskPhase {
    match phase {
        ResTaskPhase::Pending => WireTaskPhase::Pending,
        ResTaskPhase::Ready => WireTaskPhase::Ready,
        ResTaskPhase::Launching => WireTaskPhase::Launching,
        ResTaskPhase::Running => WireTaskPhase::Running,
        ResTaskPhase::Completed => WireTaskPhase::Completed,
        ResTaskPhase::Failed => WireTaskPhase::Failed,
        ResTaskPhase::Cancelled => WireTaskPhase::Cancelled,
    }
}

#[allow(dead_code)]
fn summarize_task(def: &SnapshotTask, state: Option<&TaskState>) -> TaskSummary {
    let phase = state.map(|s| wire_task_phase(s.phase)).unwrap_or(WireTaskPhase::Pending);
    TaskSummary {
        name: def.name.clone(),
        depends_on: def.depends_on.clone(),
        phase,
        processes: def
            .processes
            .iter()
            .map(|p| {
                let command_preview = match &p.source {
                    ProcessSource::Tool { command } => command.clone(),
                    ProcessSource::Agent { selector, prompt } => {
                        prompt.clone().unwrap_or_else(|| selector.capability.clone())
                    }
                };
                ProcessSummary { role: p.role.clone(), command_preview }
            })
            .collect(),
        host: None,
        checkout: None,
        workspace_ref: None,
        ready_at: state.and_then(|s| s.ready_at),
        started_at: state.and_then(|s| s.started_at),
        finished_at: state.and_then(|s| s.finished_at),
        message: state.and_then(|s| s.message.clone()),
    }
}

#[allow(dead_code)]
pub(crate) fn summarize_convoy(convoy: &ResourceObject<Convoy>) -> ConvoySummary {
    let namespace = convoy.metadata.namespace.clone();
    let name = convoy.metadata.name.clone();
    let id = ConvoyId::new(&namespace, &name);

    let status = convoy.status.as_ref();

    let tasks: Vec<TaskSummary> = status
        .and_then(|s| s.workflow_snapshot.as_ref())
        .map(|snap| {
            snap.tasks
                .iter()
                .map(|t| summarize_task(t, status.and_then(|s| s.tasks.get(&t.name))))
                .collect()
        })
        .unwrap_or_default();

    let initializing = status.map(|s| s.workflow_snapshot.is_none()).unwrap_or(true);

    ConvoySummary {
        id,
        namespace,
        name,
        workflow_ref: convoy.spec.workflow_ref.clone(),
        phase: wire_convoy_phase(status.map(|s| s.phase).unwrap_or_default()),
        message: status.and_then(|s| s.message.clone()),
        repo_hint: None,
        tasks,
        started_at: status.and_then(|s| s.started_at),
        finished_at: status.and_then(|s| s.finished_at),
        observed_workflow_ref: status.and_then(|s| s.observed_workflow_ref.clone()),
        initializing,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use flotilla_resources::{
        ConvoyPhase as ResConvoyPhase, ConvoySpec, ConvoyStatus, ObjectMeta, ProcessDefinition, ProcessSource, ResourceObject,
        SnapshotTask, TaskPhase as ResTaskPhase, TaskState, WorkflowSnapshot,
    };

    use super::*;

    fn meta(ns: &str, name: &str) -> ObjectMeta {
        ObjectMeta {
            name: name.into(),
            namespace: ns.into(),
            resource_version: "1".into(),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            owner_references: vec![],
            finalizers: vec![],
            deletion_timestamp: None,
            creation_timestamp: Utc::now(),
        }
    }

    fn task_state(phase: ResTaskPhase) -> TaskState {
        TaskState { phase, ready_at: None, started_at: None, finished_at: None, message: None, placement: None }
    }

    #[test]
    fn summarize_convoy_builds_full_summary_when_snapshot_present() {
        let convoy = ResourceObject {
            metadata: meta("flotilla", "fix-bug-123"),
            spec: ConvoySpec {
                workflow_ref: "review-and-fix".into(),
                inputs: BTreeMap::new(),
                placement_policy: None,
                repository: None,
                r#ref: None,
            },
            status: Some(ConvoyStatus {
                phase: ResConvoyPhase::Active,
                workflow_snapshot: Some(WorkflowSnapshot {
                    tasks: vec![SnapshotTask {
                        name: "implement".into(),
                        depends_on: vec![],
                        processes: vec![ProcessDefinition {
                            role: "coder".into(),
                            source: ProcessSource::Tool { command: "claude".into() },
                            labels: BTreeMap::new(),
                        }],
                    }],
                }),
                tasks: std::iter::once(("implement".into(), task_state(ResTaskPhase::Running)))
                    .collect::<BTreeMap<_, _>>(),
                observed_workflow_ref: Some("review-and-fix".into()),
                ..Default::default()
            }),
        };
        let summary = summarize_convoy(&convoy);
        assert_eq!(summary.namespace, "flotilla");
        assert_eq!(summary.name, "fix-bug-123");
        assert_eq!(summary.workflow_ref, "review-and-fix");
        assert!(matches!(summary.phase, flotilla_protocol::namespace::ConvoyPhase::Active));
        assert!(!summary.initializing, "snapshot present → not initializing");
        assert_eq!(summary.tasks.len(), 1);
        assert_eq!(summary.tasks[0].name, "implement");
    }

    #[test]
    fn summarize_convoy_marks_initializing_when_snapshot_absent() {
        let convoy = ResourceObject {
            metadata: meta("flotilla", "new-one"),
            spec: ConvoySpec {
                workflow_ref: "wf".into(),
                inputs: BTreeMap::new(),
                placement_policy: None,
                repository: None,
                r#ref: None,
            },
            status: Some(ConvoyStatus {
                phase: ResConvoyPhase::Pending,
                workflow_snapshot: None,
                tasks: Default::default(),
                ..Default::default()
            }),
        };
        let summary = summarize_convoy(&convoy);
        assert!(summary.initializing);
        assert!(summary.tasks.is_empty());
    }
}
