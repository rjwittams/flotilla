mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use common::{
    controller_meta, create_convoy_with_single_task, create_host_direct_policy, create_ready_clone, create_ready_host_direct_environment,
    create_workspace, ControllerLoopHarness,
};
use flotilla_controllers::reconcilers::{
    CheckoutReconciler, CheckoutRuntime, CloneReconciler, CloneRuntime, DockerEnvironmentRuntime, EnvironmentReconciler,
    TaskWorkspaceReconciler, TerminalRuntime, TerminalRuntimeState, TerminalSessionReconciler,
};
use flotilla_resources::{
    clone_key, controller::ControllerLoop, Checkout, CheckoutPhase, CheckoutSpec, CheckoutWorktreeSpec, Clone, ClonePhase, CloneSpec,
    DockerEnvironmentSpec, Environment, EnvironmentMount, EnvironmentMountMode, EnvironmentPhase, EnvironmentSpec, Host,
    HostDirectEnvironmentSpec, HostSpec, HostStatus, ResourceBackend, ResourceError, TaskWorkspace, TaskWorkspacePhase,
    TerminalSession, TerminalSessionPhase, TASK_WORKSPACE_LABEL,
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
    create_ready_host_direct_environment(&backend, NAMESPACE, "01HXYZ", "/Users/alice/dev/flotilla-repos").await;
    create_host_direct_policy(&backend, NAMESPACE, "policy-a", "01HXYZ", "cleat").await;
    create_convoy_with_single_task(
        &backend,
        NAMESPACE,
        "convoy-a",
        "implement",
        "git@github.com:flotilla-org/flotilla.git",
        "feat/task-provisioning",
    )
    .await;
    create_workspace(&backend, NAMESPACE, "workspace-a", "convoy-a", "implement", "policy-a", "git@github.com:flotilla-org/flotilla.git")
        .await;

    let harness = full_controller_harness(backend.clone());

    let workspaces = backend.clone().using::<TaskWorkspace>(NAMESPACE);
    harness
        .wait_until(Duration::from_secs(3), || {
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

    harness.shutdown().await;
}

#[tokio::test]
async fn clone_controller_marks_clone_ready() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_ready_host_direct_environment(&backend, NAMESPACE, "01HXYZ", "/Users/alice/dev/flotilla-repos").await;

    let clones = backend.clone().using::<Clone>(NAMESPACE);
    let clone_name = format!("clone-{}", clone_key("https://github.com/flotilla-org/flotilla", "host-direct-01HXYZ"));
    clones
        .create(&controller_meta().name(&clone_name).call(), &CloneSpec {
            url: "git@github.com:flotilla-org/flotilla.git".to_string(),
            env_ref: "host-direct-01HXYZ".to_string(),
            path: "/Users/alice/dev/flotilla".to_string(),
        })
        .await
        .expect("clone create should succeed");

    let harness = clone_harness(backend.clone());
    harness
        .wait_until(Duration::from_secs(1), || {
            let clones = clones.clone();
            let clone_name = clone_name.clone();
            async move {
                matches!(
                    clones.get(&clone_name).await.ok().and_then(|clone| clone.status).map(|status| status.phase),
                    Some(ClonePhase::Ready)
                )
            }
        })
        .await;

    let clone = clones.get(&clone_name).await.expect("clone get should succeed");
    let status = clone.status.expect("clone status should be present");
    assert_eq!(status.phase, flotilla_resources::ClonePhase::Ready);
    assert_eq!(status.default_branch.as_deref(), Some("main"));

    harness.shutdown().await;
}

#[tokio::test]
async fn environment_controller_marks_docker_environment_ready() {
    let backend = ResourceBackend::InMemory(Default::default());
    let environments = backend.clone().using::<Environment>(NAMESPACE);
    environments
        .create(&controller_meta().name("docker-env").call(), &EnvironmentSpec {
            host_direct: None,
            docker: Some(DockerEnvironmentSpec {
                host_ref: "01HXYZ".to_string(),
                image: "ghcr.io/flotilla/dev:latest".to_string(),
                mounts: vec![EnvironmentMount {
                    source_path: "/tmp/src".to_string(),
                    target_path: "/workspace".to_string(),
                    mode: EnvironmentMountMode::Rw,
                }],
                env: Default::default(),
            }),
        })
        .await
        .expect("environment create should succeed");

    let harness = environment_harness(backend.clone());
    harness
        .wait_until(Duration::from_secs(1), || {
            let environments = environments.clone();
            async move {
                matches!(
                    environments.get("docker-env").await.ok().and_then(|environment| environment.status),
                    Some(status)
                        if status.phase == EnvironmentPhase::Ready
                            && status.docker_container_id.as_deref() == Some("container-docker-env")
                )
            }
        })
        .await;

    harness.shutdown().await;
}

#[tokio::test]
async fn checkout_controller_marks_worktree_checkout_ready() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_ready_clone(
        &backend,
        NAMESPACE,
        "clone-a",
        "git@github.com:flotilla-org/flotilla.git",
        "host-direct-01HXYZ",
        "/Users/alice/dev/flotilla",
    )
    .await;
    let checkouts = backend.clone().using::<Checkout>(NAMESPACE);
    checkouts
        .create(&controller_meta().name("checkout-a").call(), &flotilla_resources::CheckoutSpec {
            env_ref: "host-direct-01HXYZ".to_string(),
            r#ref: "feat/convoy-resource".to_string(),
            target_path: "/Users/alice/dev/flotilla.feat-123".to_string(),
            worktree: Some(CheckoutWorktreeSpec { clone_ref: "clone-a".to_string() }),
            fresh_clone: None,
        })
        .await
        .expect("checkout create should succeed");

    let harness = checkout_harness(backend.clone());
    harness
        .wait_until(Duration::from_secs(1), || {
            let checkouts = checkouts.clone();
            async move {
                matches!(
                    checkouts.get("checkout-a").await.ok().and_then(|checkout| checkout.status),
                    Some(status)
                        if status.phase == CheckoutPhase::Ready
                            && status.path.as_deref() == Some("/Users/alice/dev/flotilla.feat-123")
                            && status.commit.as_deref() == Some("44982740")
                )
            }
        })
        .await;

    harness.shutdown().await;
}

#[tokio::test]
async fn terminal_session_controller_marks_session_running() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_ready_host_direct_environment(&backend, NAMESPACE, "01HXYZ", "/Users/alice/dev/flotilla-repos").await;
    let sessions = backend.clone().using::<TerminalSession>(NAMESPACE);
    sessions
        .create(&controller_meta().name("term-a").call(), &flotilla_resources::TerminalSessionSpec {
            env_ref: "host-direct-01HXYZ".to_string(),
            role: "coder".to_string(),
            command: "cargo test".to_string(),
            cwd: "/workspace".to_string(),
            pool: "cleat".to_string(),
        })
        .await
        .expect("session create should succeed");

    let harness = terminal_harness(backend.clone());
    harness
        .wait_until(Duration::from_secs(1), || {
            let sessions = sessions.clone();
            async move {
                matches!(
                    sessions.get("term-a").await.ok().and_then(|session| session.status),
                    Some(status)
                        if status.phase == TerminalSessionPhase::Running
                            && status.session_id.as_deref() == Some("session-term-a")
                            && status.pid == Some(42)
                )
            }
        })
        .await;

    harness.shutdown().await;
}

#[tokio::test]
async fn task_workspace_controller_finalizer_deletes_labeled_children_on_delete() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_workspace(
        &backend,
        NAMESPACE,
        "workspace-delete",
        "convoy-delete",
        "implement",
        "policy-delete",
        "git@github.com:flotilla-org/flotilla.git",
    )
    .await;

    backend
        .clone()
        .using::<Environment>(NAMESPACE)
        .create(
            &controller_meta()
                .name("env-workspace-delete")
                .labels([(TASK_WORKSPACE_LABEL.to_string(), "workspace-delete".to_string())].into_iter().collect())
                .call(),
            &EnvironmentSpec {
                host_direct: Some(HostDirectEnvironmentSpec {
                    host_ref: "01HXYZ".to_string(),
                    repo_default_dir: "/Users/alice/dev/flotilla-repos".to_string(),
                }),
                docker: None,
            },
        )
        .await
        .expect("environment create should succeed");
    backend
        .clone()
        .using::<Checkout>(NAMESPACE)
        .create(
            &controller_meta()
                .name("checkout-workspace-delete")
                .labels([(TASK_WORKSPACE_LABEL.to_string(), "workspace-delete".to_string())].into_iter().collect())
                .call(),
            &CheckoutSpec {
                env_ref: "host-direct-01HXYZ".to_string(),
                r#ref: "feat/task-provisioning".to_string(),
                target_path: "/Users/alice/dev/flotilla-repos/workspace-delete".to_string(),
                worktree: Some(CheckoutWorktreeSpec { clone_ref: "clone-a".to_string() }),
                fresh_clone: None,
            },
        )
        .await
        .expect("checkout create should succeed");
    backend
        .clone()
        .using::<TerminalSession>(NAMESPACE)
        .create(
            &controller_meta()
                .name("terminal-workspace-delete-coder")
                .labels([(TASK_WORKSPACE_LABEL.to_string(), "workspace-delete".to_string())].into_iter().collect())
                .call(),
            &flotilla_resources::TerminalSessionSpec {
                env_ref: "host-direct-01HXYZ".to_string(),
                role: "coder".to_string(),
                command: "cargo test".to_string(),
                cwd: "/Users/alice/dev/flotilla-repos/workspace-delete".to_string(),
                pool: "cleat".to_string(),
            },
        )
        .await
        .expect("terminal create should succeed");

    let workspaces = backend.clone().using::<TaskWorkspace>(NAMESPACE);
    let environments = backend.clone().using::<Environment>(NAMESPACE);
    let checkouts = backend.clone().using::<Checkout>(NAMESPACE);
    let terminals = backend.clone().using::<TerminalSession>(NAMESPACE);
    let mut harness = ControllerLoopHarness::new(backend.clone());
    harness.spawn(
        ControllerLoop {
            primary: workspaces.clone(),
            secondaries: TaskWorkspaceReconciler::secondary_watches(),
            reconciler: TaskWorkspaceReconciler::new(backend.clone(), NAMESPACE),
            resync_interval: Duration::from_millis(50),
            backend: backend.clone(),
        }
        .run(),
    );

    harness
        .wait_until(Duration::from_secs(1), || {
            let workspaces = workspaces.clone();
            async move {
                matches!(
                    workspaces.get("workspace-delete").await,
                    Ok(workspace) if workspace.metadata.finalizers == vec!["flotilla.work/task-workspace-teardown".to_string()]
                )
            }
        })
        .await;

    workspaces.delete("workspace-delete").await.expect("workspace delete should succeed");

    harness
        .wait_until(Duration::from_secs(1), || {
            let workspaces = workspaces.clone();
            let environments = environments.clone();
            let checkouts = checkouts.clone();
            let terminals = terminals.clone();
            async move {
                matches!(workspaces.get("workspace-delete").await, Err(ResourceError::NotFound { .. }))
                    && matches!(environments.get("env-workspace-delete").await, Err(ResourceError::NotFound { .. }))
                    && matches!(checkouts.get("checkout-workspace-delete").await, Err(ResourceError::NotFound { .. }))
                    && matches!(terminals.get("terminal-workspace-delete-coder").await, Err(ResourceError::NotFound { .. }))
            }
        })
        .await;

    harness.shutdown().await;
}

fn environment_harness(backend: ResourceBackend) -> ControllerLoopHarness {
    let mut harness = ControllerLoopHarness::new(backend.clone());
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<Environment>(NAMESPACE),
            secondaries: vec![],
            reconciler: EnvironmentReconciler::new(Arc::new(FakeDockerRuntime::default())),
            resync_interval: Duration::from_millis(50),
            backend,
        }
        .run(),
    );
    harness
}

fn clone_harness(backend: ResourceBackend) -> ControllerLoopHarness {
    let mut harness = ControllerLoopHarness::new(backend.clone());
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<Clone>(NAMESPACE),
            secondaries: vec![],
            reconciler: CloneReconciler::new(Arc::new(FakeCloneRuntime)),
            resync_interval: Duration::from_millis(50),
            backend,
        }
        .run(),
    );
    harness
}

fn checkout_harness(backend: ResourceBackend) -> ControllerLoopHarness {
    let mut harness = ControllerLoopHarness::new(backend.clone());
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<Checkout>(NAMESPACE),
            secondaries: vec![],
            reconciler: CheckoutReconciler::new(Arc::new(FakeCheckoutRuntime), backend.clone(), NAMESPACE),
            resync_interval: Duration::from_millis(50),
            backend,
        }
        .run(),
    );
    harness
}

fn terminal_harness(backend: ResourceBackend) -> ControllerLoopHarness {
    let mut harness = ControllerLoopHarness::new(backend.clone());
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<TerminalSession>(NAMESPACE),
            secondaries: vec![],
            reconciler: TerminalSessionReconciler::new(Arc::new(FakeTerminalRuntime), backend.clone(), NAMESPACE),
            resync_interval: Duration::from_millis(50),
            backend,
        }
        .run(),
    );
    harness
}

fn full_controller_harness(backend: ResourceBackend) -> ControllerLoopHarness {
    let mut harness = environment_harness(backend.clone());
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<Clone>(NAMESPACE),
            secondaries: vec![],
            reconciler: CloneReconciler::new(Arc::new(FakeCloneRuntime)),
            resync_interval: Duration::from_millis(50),
            backend: backend.clone(),
        }
        .run(),
    );
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<Checkout>(NAMESPACE),
            secondaries: vec![],
            reconciler: CheckoutReconciler::new(Arc::new(FakeCheckoutRuntime), backend.clone(), NAMESPACE),
            resync_interval: Duration::from_millis(50),
            backend: backend.clone(),
        }
        .run(),
    );
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<TerminalSession>(NAMESPACE),
            secondaries: vec![],
            reconciler: TerminalSessionReconciler::new(Arc::new(FakeTerminalRuntime), backend.clone(), NAMESPACE),
            resync_interval: Duration::from_millis(50),
            backend: backend.clone(),
        }
        .run(),
    );
    harness.spawn(
        ControllerLoop {
            primary: backend.clone().using::<TaskWorkspace>(NAMESPACE),
            secondaries: TaskWorkspaceReconciler::secondary_watches(),
            reconciler: TaskWorkspaceReconciler::new(backend.clone(), NAMESPACE),
            resync_interval: Duration::from_millis(50),
            backend,
        }
        .run(),
    );
    harness
}

async fn create_ready_host(backend: &ResourceBackend, name: &str) {
    let hosts = backend.clone().using::<Host>(NAMESPACE);
    let created = hosts.create(&controller_meta().name(name).call(), &HostSpec {}).await.expect("host create should succeed");
    hosts
        .update_status(name, &created.metadata.resource_version, &HostStatus {
            capabilities: Default::default(),
            heartbeat_at: Some(Utc::now()),
            ready: true,
        })
        .await
        .expect("host status update should succeed");
}
