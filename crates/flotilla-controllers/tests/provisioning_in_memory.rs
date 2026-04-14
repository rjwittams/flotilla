mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use common::{meta, task_workspace_meta, wait_until};
use flotilla_controllers::reconcilers::{
    CheckoutReconciler, CheckoutRuntime, CloneReconciler, CloneRuntime, DockerEnvironmentRuntime, EnvironmentReconciler,
    TaskWorkspaceReconciler, TerminalRuntime, TerminalRuntimeState, TerminalSessionReconciler,
};
use flotilla_resources::{
    controller::ControllerLoop, Convoy, ConvoyRepositorySpec, ConvoySpec, ConvoyStatus, Environment, EnvironmentPhase, EnvironmentSpec,
    EnvironmentStatus, Host, HostDirectEnvironmentSpec, HostDirectPlacementPolicyCheckout, HostDirectPlacementPolicySpec, HostSpec,
    HostStatus, PlacementPolicy, PlacementPolicySpec, ProcessDefinition, ProcessSource, ResourceBackend, SnapshotTask, TaskWorkspace,
    TaskWorkspacePhase, TaskWorkspaceSpec, WorkflowSnapshot,
};

const NAMESPACE: &str = "flotilla";

#[derive(Default)]
struct FakeDockerRuntime {
    destroyed: Mutex<Vec<String>>,
}

#[async_trait]
impl DockerEnvironmentRuntime for FakeDockerRuntime {
    async fn provision(&self, name: &str, _spec: &flotilla_resources::DockerEnvironmentSpec) -> Result<String, String> {
        Ok(format!("container-{name}"))
    }

    async fn destroy(&self, container_id: &str) -> Result<(), String> {
        self.destroyed.lock().expect("destroyed lock").push(container_id.to_string());
        Ok(())
    }
}

#[derive(Default)]
struct FakeCloneRuntime;

#[async_trait]
impl CloneRuntime for FakeCloneRuntime {
    async fn clone_and_inspect(&self, _repo_url: &str, _target_path: &str) -> Result<Option<String>, String> {
        Ok(Some("main".to_string()))
    }

    async fn inspect_existing(&self, _target_path: &str) -> Result<Option<String>, String> {
        Ok(Some("main".to_string()))
    }
}

#[derive(Default)]
struct FakeCheckoutRuntime;

#[async_trait]
impl CheckoutRuntime for FakeCheckoutRuntime {
    async fn create_worktree(&self, _clone_path: &str, _branch: &str, _target_path: &str) -> Result<Option<String>, String> {
        Ok(Some("44982740".to_string()))
    }

    async fn create_fresh_clone(&self, _repo_url: &str, _branch: &str, _target_path: &str) -> Result<Option<String>, String> {
        Ok(Some("44982740".to_string()))
    }

    async fn remove_checkout(&self, _target_path: &str) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct FakeTerminalRuntime;

#[async_trait]
impl TerminalRuntime for FakeTerminalRuntime {
    async fn ensure_session(&self, name: &str, _spec: &flotilla_resources::TerminalSessionSpec) -> Result<TerminalRuntimeState, String> {
        Ok(TerminalRuntimeState { session_id: format!("session-{name}"), pid: Some(42), started_at: Utc::now() })
    }

    async fn kill_session(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn controller_loops_drive_host_direct_workspace_to_ready() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_ready_host(&backend, "01HXYZ").await;
    create_ready_host_direct_environment(&backend, "01HXYZ").await;
    create_policy(&backend, "policy-a").await;
    create_convoy(&backend, "convoy-a", "implement").await;
    create_workspace(&backend, "workspace-a", "convoy-a", "implement", "policy-a").await;

    let handles = vec![
        tokio::spawn(
            ControllerLoop {
                primary: backend.clone().using::<Environment>(NAMESPACE),
                secondaries: vec![],
                reconciler: EnvironmentReconciler::new(Arc::new(FakeDockerRuntime::default())),
                resync_interval: Duration::from_millis(50),
                backend: backend.clone(),
            }
            .run(),
        ),
        tokio::spawn(
            ControllerLoop {
                primary: backend.clone().using::<flotilla_resources::Clone>(NAMESPACE),
                secondaries: vec![],
                reconciler: CloneReconciler::new(Arc::new(FakeCloneRuntime)),
                resync_interval: Duration::from_millis(50),
                backend: backend.clone(),
            }
            .run(),
        ),
        tokio::spawn(
            ControllerLoop {
                primary: backend.clone().using::<flotilla_resources::Checkout>(NAMESPACE),
                secondaries: vec![],
                reconciler: CheckoutReconciler::new(Arc::new(FakeCheckoutRuntime), backend.clone(), NAMESPACE),
                resync_interval: Duration::from_millis(50),
                backend: backend.clone(),
            }
            .run(),
        ),
        tokio::spawn(
            ControllerLoop {
                primary: backend.clone().using::<flotilla_resources::TerminalSession>(NAMESPACE),
                secondaries: vec![],
                reconciler: TerminalSessionReconciler::new(Arc::new(FakeTerminalRuntime), backend.clone(), NAMESPACE),
                resync_interval: Duration::from_millis(50),
                backend: backend.clone(),
            }
            .run(),
        ),
        tokio::spawn(
            ControllerLoop {
                primary: backend.clone().using::<TaskWorkspace>(NAMESPACE),
                secondaries: TaskWorkspaceReconciler::secondary_watches(),
                reconciler: TaskWorkspaceReconciler::new(backend.clone(), NAMESPACE),
                resync_interval: Duration::from_millis(50),
                backend: backend.clone(),
            }
            .run(),
        ),
    ];

    let workspaces = backend.clone().using::<TaskWorkspace>(NAMESPACE);
    wait_until(Duration::from_secs(3), || {
        let workspaces = workspaces.clone();
        async move {
            matches!(
                workspaces.get("workspace-a").await.ok().and_then(|workspace| workspace.status).map(|status| status.phase),
                Some(TaskWorkspacePhase::Ready)
            )
        }
    })
    .await;

    let workspace = workspaces.get("workspace-a").await.expect("workspace get should succeed");
    let status = workspace.status.expect("workspace status should be present");
    assert_eq!(status.phase, TaskWorkspacePhase::Ready);
    assert_eq!(status.environment_ref.as_deref(), Some("host-direct-01HXYZ"));
    assert_eq!(status.checkout_ref.as_deref(), Some("checkout-workspace-a"));
    assert_eq!(status.terminal_session_refs, vec!["terminal-workspace-a-coder".to_string()]);

    for handle in handles {
        handle.abort();
        let _ = handle.await;
    }
}

async fn create_ready_host(backend: &ResourceBackend, name: &str) {
    let hosts = backend.clone().using::<Host>(NAMESPACE);
    let created = hosts.create(&meta(name), &HostSpec {}).await.expect("host create should succeed");
    hosts
        .update_status(name, &created.metadata.resource_version, &HostStatus {
            capabilities: Default::default(),
            heartbeat_at: Some(Utc::now()),
            ready: true,
        })
        .await
        .expect("host status update should succeed");
}

async fn create_ready_host_direct_environment(backend: &ResourceBackend, host_ref: &str) {
    let environments = backend.clone().using::<Environment>(NAMESPACE);
    let name = format!("host-direct-{host_ref}");
    let created = environments
        .create(&meta(&name), &EnvironmentSpec {
            host_direct: Some(HostDirectEnvironmentSpec {
                host_ref: host_ref.to_string(),
                repo_default_dir: "/Users/alice/dev/flotilla-repos".to_string(),
            }),
            docker: None,
        })
        .await
        .expect("environment create should succeed");
    environments
        .update_status(&name, &created.metadata.resource_version, &EnvironmentStatus {
            phase: EnvironmentPhase::Ready,
            ready: true,
            docker_container_id: None,
            message: None,
        })
        .await
        .expect("environment status update should succeed");
}

async fn create_policy(backend: &ResourceBackend, name: &str) {
    backend
        .clone()
        .using::<PlacementPolicy>(NAMESPACE)
        .create(&meta(name), &PlacementPolicySpec {
            pool: "cleat".to_string(),
            host_direct: Some(HostDirectPlacementPolicySpec {
                host_ref: "01HXYZ".to_string(),
                checkout: HostDirectPlacementPolicyCheckout::Worktree,
            }),
            docker_per_task: None,
        })
        .await
        .expect("policy create should succeed");
}

async fn create_convoy(backend: &ResourceBackend, name: &str, task: &str) {
    let convoys = backend.clone().using::<Convoy>(NAMESPACE);
    let convoy = convoys
        .create(&meta(name), &ConvoySpec {
            workflow_ref: "wf".to_string(),
            inputs: Default::default(),
            placement_policy: None,
            repository: Some(ConvoyRepositorySpec { url: "git@github.com:flotilla-org/flotilla.git".to_string() }),
            r#ref: Some("feat/task-provisioning".to_string()),
        })
        .await
        .expect("convoy create should succeed");
    convoys
        .update_status(name, &convoy.metadata.resource_version, &ConvoyStatus {
            workflow_snapshot: Some(WorkflowSnapshot {
                tasks: vec![SnapshotTask {
                    name: task.to_string(),
                    depends_on: Vec::new(),
                    processes: vec![ProcessDefinition {
                        role: "coder".to_string(),
                        source: ProcessSource::Tool { command: "cargo test".to_string() },
                    }],
                }],
            }),
            ..Default::default()
        })
        .await
        .expect("convoy status update should succeed");
}

async fn create_workspace(backend: &ResourceBackend, name: &str, convoy_ref: &str, task: &str, placement_policy_ref: &str) {
    backend
        .clone()
        .using::<TaskWorkspace>(NAMESPACE)
        .create(&task_workspace_meta(name, "git@github.com:flotilla-org/flotilla.git"), &TaskWorkspaceSpec {
            convoy_ref: convoy_ref.to_string(),
            task: task.to_string(),
            placement_policy_ref: placement_policy_ref.to_string(),
        })
        .await
        .expect("workspace create should succeed");
}
