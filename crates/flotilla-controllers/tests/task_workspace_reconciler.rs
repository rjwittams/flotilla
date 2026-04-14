mod common;

use common::{meta, task_workspace_meta};
use flotilla_controllers::reconcilers::TaskWorkspaceReconciler;
use flotilla_resources::{
    canonicalize_repo_url, clone_key,
    controller::{Actuation, Reconciler},
    CheckoutPhase, CheckoutSpec, CheckoutStatus, CheckoutWorktreeSpec, ClonePhase, CloneSpec, CloneStatus, Convoy, ConvoyRepositorySpec,
    ConvoySpec, ConvoyStatus, DockerCheckoutStrategy, DockerEnvironmentSpec, DockerPerTaskPlacementPolicySpec, Environment,
    EnvironmentPhase, EnvironmentSpec, EnvironmentStatus, HostDirectEnvironmentSpec, HostDirectPlacementPolicyCheckout,
    HostDirectPlacementPolicySpec, PlacementPolicy, PlacementPolicySpec, ProcessDefinition, ProcessSource, ResourceBackend, SnapshotTask,
    TaskWorkspace, TaskWorkspaceSpec, TerminalSessionPhase, TerminalSessionStatus, WorkflowSnapshot,
};

const NAMESPACE: &str = "flotilla";
const REPO_URL: &str = "git@github.com:flotilla-org/flotilla.git";
const GIT_REF: &str = "feat/task-provisioning";
const HOST_REF: &str = "01HXYZ";

#[tokio::test]
async fn missing_placement_policy_marks_workspace_failed() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_convoy_with_single_task(&backend, "convoy-a", "implement").await;
    let workspace = create_workspace(&backend, "workspace-a", "convoy-a", "implement", "policy-missing").await;

    let reconciler = TaskWorkspaceReconciler::new(backend, NAMESPACE);
    let deps = reconciler.fetch_dependencies(&workspace).await.expect("deps should load");
    let outcome = reconciler.reconcile(&workspace, &deps, chrono::Utc::now());

    assert!(matches!(
        outcome.patch,
        Some(flotilla_resources::TaskWorkspaceStatusPatch::MarkFailed { ref message })
            if message.contains("placement policy policy-missing not found")
    ));
}

#[tokio::test]
async fn reuses_existing_clone_by_deterministic_name() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_convoy_with_single_task(&backend, "convoy-b", "implement").await;
    create_host_direct_policy(&backend, "policy-a").await;
    create_ready_host_direct_environment(&backend).await;

    let canonical_repo = canonicalize_repo_url(REPO_URL).expect("repo canonicalization");
    let clone_name = format!("clone-{}", clone_key(&canonical_repo, &host_direct_env_name()));
    create_ready_clone(&backend, &clone_name).await;
    let workspace = create_workspace(&backend, "workspace-b", "convoy-b", "implement", "policy-a").await;

    let reconciler = TaskWorkspaceReconciler::new(backend, NAMESPACE);
    let deps = reconciler.fetch_dependencies(&workspace).await.expect("deps should load");
    let outcome = reconciler.reconcile(&workspace, &deps, chrono::Utc::now());

    assert!(outcome.actuations.iter().all(|actuation| !matches!(actuation, Actuation::CreateClone { .. })));
    assert!(outcome.actuations.iter().any(|actuation| {
        matches!(
            actuation,
            Actuation::CreateCheckout { spec, .. }
                if spec.worktree.as_ref().map(|worktree| worktree.clone_ref.as_str()) == Some(clone_name.as_str())
        )
    }));
}

#[tokio::test]
async fn docker_worktree_waits_for_checkout_before_creating_environment() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_convoy_with_single_task(&backend, "convoy-c", "implement").await;
    create_docker_worktree_policy(&backend, "policy-worktree", "/workspace", None).await;
    create_ready_host_direct_environment(&backend).await;

    let canonical_repo = canonicalize_repo_url(REPO_URL).expect("repo canonicalization");
    let clone_name = format!("clone-{}", clone_key(&canonical_repo, &host_direct_env_name()));
    create_ready_clone(&backend, &clone_name).await;
    let workspace = create_workspace(&backend, "workspace-c", "convoy-c", "implement", "policy-worktree").await;

    let reconciler = TaskWorkspaceReconciler::new(backend.clone(), NAMESPACE);
    let deps = reconciler.fetch_dependencies(&workspace).await.expect("deps should load");
    let outcome = reconciler.reconcile(&workspace, &deps, chrono::Utc::now());
    assert!(outcome.actuations.iter().any(|actuation| matches!(actuation, Actuation::CreateCheckout { .. })));
    assert!(outcome.actuations.iter().all(|actuation| !matches!(actuation, Actuation::CreateEnvironment { .. })));

    create_ready_checkout(
        &backend,
        "checkout-workspace-c",
        &host_direct_env_name(),
        "/Users/alice/dev/flotilla-repos/github-com-flotilla-org-flotilla.workspace-c",
    )
    .await;
    let current = backend.clone().using::<TaskWorkspace>(NAMESPACE).get("workspace-c").await.expect("workspace get should succeed");
    let deps = reconciler.fetch_dependencies(&current).await.expect("deps should reload");
    let outcome = reconciler.reconcile(&current, &deps, chrono::Utc::now());

    assert!(outcome.actuations.iter().any(|actuation| {
        matches!(
            actuation,
            Actuation::CreateEnvironment { spec, .. }
                if spec.docker.as_ref().map(|docker| docker.mounts.as_slice()) == Some(&[flotilla_resources::EnvironmentMount {
                    source_path: "/Users/alice/dev/flotilla-repos/github-com-flotilla-org-flotilla.workspace-c".to_string(),
                    target_path: "/workspace".to_string(),
                    mode: flotilla_resources::EnvironmentMountMode::Rw,
                }])
        )
    }));
}

#[tokio::test]
async fn terminal_sessions_use_strategy_specific_cwd() {
    assert_terminal_cwd_for_strategy(
        "workspace-host",
        PlacementPolicySpec {
            pool: "cleat".to_string(),
            host_direct: Some(HostDirectPlacementPolicySpec {
                host_ref: HOST_REF.to_string(),
                checkout: HostDirectPlacementPolicyCheckout::Worktree,
            }),
            docker_per_task: None,
        },
        "/Users/alice/dev/flotilla-repos/github-com-flotilla-org-flotilla.workspace-host",
        "/Users/alice/dev/flotilla-repos/github-com-flotilla-org-flotilla.workspace-host",
        None,
    )
    .await;

    assert_terminal_cwd_for_strategy(
        "workspace-docker-worktree",
        PlacementPolicySpec {
            pool: "cleat".to_string(),
            host_direct: None,
            docker_per_task: Some(DockerPerTaskPlacementPolicySpec {
                host_ref: HOST_REF.to_string(),
                image: "ghcr.io/flotilla/dev:latest".to_string(),
                default_cwd: None,
                env: Default::default(),
                checkout: DockerCheckoutStrategy::WorktreeOnHostAndMount { mount_path: "/workspace".to_string() },
            }),
        },
        "/Users/alice/dev/flotilla-repos/github-com-flotilla-org-flotilla.workspace-docker-worktree",
        "/workspace",
        Some(DockerEnvironmentSpec {
            host_ref: HOST_REF.to_string(),
            image: "ghcr.io/flotilla/dev:latest".to_string(),
            mounts: vec![flotilla_resources::EnvironmentMount {
                source_path: "/Users/alice/dev/flotilla-repos/github-com-flotilla-org-flotilla.workspace-docker-worktree".to_string(),
                target_path: "/workspace".to_string(),
                mode: flotilla_resources::EnvironmentMountMode::Rw,
            }],
            env: Default::default(),
        }),
    )
    .await;

    assert_terminal_cwd_for_strategy(
        "workspace-docker-fresh",
        PlacementPolicySpec {
            pool: "cleat".to_string(),
            host_direct: None,
            docker_per_task: Some(DockerPerTaskPlacementPolicySpec {
                host_ref: HOST_REF.to_string(),
                image: "ghcr.io/flotilla/dev:latest".to_string(),
                default_cwd: Some("/app".to_string()),
                env: Default::default(),
                checkout: DockerCheckoutStrategy::FreshCloneInContainer { clone_path: "/workspace".to_string() },
            }),
        },
        "/workspace",
        "/app",
        Some(DockerEnvironmentSpec {
            host_ref: HOST_REF.to_string(),
            image: "ghcr.io/flotilla/dev:latest".to_string(),
            mounts: Vec::new(),
            env: Default::default(),
        }),
    )
    .await;
}

#[tokio::test]
async fn child_failure_propagates_to_workspace_failure() {
    let backend = ResourceBackend::InMemory(Default::default());
    create_convoy_with_single_task(&backend, "convoy-f", "implement").await;
    create_host_direct_policy(&backend, "policy-f").await;
    create_ready_host_direct_environment(&backend).await;

    let canonical_repo = canonicalize_repo_url(REPO_URL).expect("repo canonicalization");
    let clone_name = format!("clone-{}", clone_key(&canonical_repo, &host_direct_env_name()));
    create_ready_clone(&backend, &clone_name).await;
    create_ready_checkout(
        &backend,
        "checkout-workspace-f",
        &host_direct_env_name(),
        "/Users/alice/dev/flotilla-repos/github-com-flotilla-org-flotilla.workspace-f",
    )
    .await;
    create_stopped_terminal(&backend, "terminal-workspace-f-coder", &host_direct_env_name(), "boom").await;
    let workspace = create_workspace(&backend, "workspace-f", "convoy-f", "implement", "policy-f").await;

    let reconciler = TaskWorkspaceReconciler::new(backend, NAMESPACE);
    let deps = reconciler.fetch_dependencies(&workspace).await.expect("deps should load");
    let outcome = reconciler.reconcile(&workspace, &deps, chrono::Utc::now());

    assert!(matches!(
        outcome.patch,
        Some(flotilla_resources::TaskWorkspaceStatusPatch::MarkFailed { ref message }) if message == "boom"
    ));
}

async fn assert_terminal_cwd_for_strategy(
    workspace_name: &str,
    policy_spec: PlacementPolicySpec,
    checkout_path: &str,
    expected_cwd: &str,
    docker_env: Option<DockerEnvironmentSpec>,
) {
    let backend = ResourceBackend::InMemory(Default::default());
    create_convoy_with_single_task(&backend, "convoy-cwd", "implement").await;
    create_policy(&backend, "policy-cwd", policy_spec).await;
    create_ready_host_direct_environment(&backend).await;

    let canonical_repo = canonicalize_repo_url(REPO_URL).expect("repo canonicalization");
    let clone_name = format!("clone-{}", clone_key(&canonical_repo, &host_direct_env_name()));
    if checkout_path != "/workspace" || docker_env.is_none() {
        create_ready_clone(&backend, &clone_name).await;
    }
    if let Some(docker) = docker_env {
        create_ready_docker_environment(&backend, &format!("env-{workspace_name}"), docker).await;
    }
    let checkout_env_ref = if checkout_path == "/workspace" && workspace_name == "workspace-docker-fresh" {
        format!("env-{workspace_name}")
    } else {
        host_direct_env_name()
    };
    create_ready_checkout(&backend, &format!("checkout-{workspace_name}"), &checkout_env_ref, checkout_path).await;
    let workspace = create_workspace(&backend, workspace_name, "convoy-cwd", "implement", "policy-cwd").await;

    let reconciler = TaskWorkspaceReconciler::new(backend, NAMESPACE);
    let deps = reconciler.fetch_dependencies(&workspace).await.expect("deps should load");
    let outcome = reconciler.reconcile(&workspace, &deps, chrono::Utc::now());

    let cwd = outcome
        .actuations
        .iter()
        .find_map(|actuation| match actuation {
            Actuation::CreateTerminalSession { spec, .. } => Some(spec.cwd.as_str()),
            _ => None,
        })
        .expect("terminal actuation should be created");
    assert_eq!(cwd, expected_cwd);
}

async fn create_convoy_with_single_task(backend: &ResourceBackend, name: &str, task: &str) {
    let convoys = backend.clone().using::<Convoy>(NAMESPACE);
    let convoy = convoys
        .create(&meta(name), &ConvoySpec {
            workflow_ref: "wf".to_string(),
            inputs: Default::default(),
            placement_policy: None,
            repository: Some(ConvoyRepositorySpec { url: REPO_URL.to_string() }),
            r#ref: Some(GIT_REF.to_string()),
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

async fn create_workspace(
    backend: &ResourceBackend,
    name: &str,
    convoy_ref: &str,
    task: &str,
    placement_policy_ref: &str,
) -> flotilla_resources::ResourceObject<TaskWorkspace> {
    let workspaces = backend.clone().using::<TaskWorkspace>(NAMESPACE);
    workspaces
        .create(&task_workspace_meta(name, REPO_URL), &TaskWorkspaceSpec {
            convoy_ref: convoy_ref.to_string(),
            task: task.to_string(),
            placement_policy_ref: placement_policy_ref.to_string(),
        })
        .await
        .expect("workspace create should succeed")
}

async fn create_policy(backend: &ResourceBackend, name: &str, spec: PlacementPolicySpec) {
    backend.clone().using::<PlacementPolicy>(NAMESPACE).create(&meta(name), &spec).await.expect("policy create should succeed");
}

async fn create_host_direct_policy(backend: &ResourceBackend, name: &str) {
    create_policy(backend, name, PlacementPolicySpec {
        pool: "cleat".to_string(),
        host_direct: Some(HostDirectPlacementPolicySpec {
            host_ref: HOST_REF.to_string(),
            checkout: HostDirectPlacementPolicyCheckout::Worktree,
        }),
        docker_per_task: None,
    })
    .await;
}

async fn create_docker_worktree_policy(backend: &ResourceBackend, name: &str, mount_path: &str, default_cwd: Option<&str>) {
    create_policy(backend, name, PlacementPolicySpec {
        pool: "cleat".to_string(),
        host_direct: None,
        docker_per_task: Some(DockerPerTaskPlacementPolicySpec {
            host_ref: HOST_REF.to_string(),
            image: "ghcr.io/flotilla/dev:latest".to_string(),
            default_cwd: default_cwd.map(ToString::to_string),
            env: Default::default(),
            checkout: DockerCheckoutStrategy::WorktreeOnHostAndMount { mount_path: mount_path.to_string() },
        }),
    })
    .await;
}

async fn create_ready_host_direct_environment(backend: &ResourceBackend) {
    let environments = backend.clone().using::<Environment>(NAMESPACE);
    let name = host_direct_env_name();
    let created = environments
        .create(&meta(&name), &EnvironmentSpec {
            host_direct: Some(HostDirectEnvironmentSpec {
                host_ref: HOST_REF.to_string(),
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

async fn create_ready_docker_environment(backend: &ResourceBackend, name: &str, docker: DockerEnvironmentSpec) {
    let environments = backend.clone().using::<Environment>(NAMESPACE);
    let created = environments
        .create(&meta(name), &EnvironmentSpec { host_direct: None, docker: Some(docker) })
        .await
        .expect("docker env create should succeed");
    environments
        .update_status(name, &created.metadata.resource_version, &EnvironmentStatus {
            phase: EnvironmentPhase::Ready,
            ready: true,
            docker_container_id: Some(format!("container-{name}")),
            message: None,
        })
        .await
        .expect("docker env status update should succeed");
}

async fn create_ready_clone(backend: &ResourceBackend, name: &str) {
    let clones = backend.clone().using::<flotilla_resources::Clone>(NAMESPACE);
    let created = clones
        .create(&meta(name), &CloneSpec {
            url: REPO_URL.to_string(),
            env_ref: host_direct_env_name(),
            path: "/Users/alice/dev/flotilla-repos/clone".to_string(),
        })
        .await
        .expect("clone create should succeed");
    clones
        .update_status(name, &created.metadata.resource_version, &CloneStatus {
            phase: ClonePhase::Ready,
            default_branch: Some("main".to_string()),
            message: None,
        })
        .await
        .expect("clone status update should succeed");
}

async fn create_ready_checkout(backend: &ResourceBackend, name: &str, env_ref: &str, path: &str) {
    let checkouts = backend.clone().using::<flotilla_resources::Checkout>(NAMESPACE);
    let created = checkouts
        .create(&meta(name), &CheckoutSpec {
            env_ref: env_ref.to_string(),
            r#ref: GIT_REF.to_string(),
            target_path: path.to_string(),
            worktree: Some(CheckoutWorktreeSpec { clone_ref: "clone-placeholder".to_string() }),
            fresh_clone: None,
        })
        .await
        .expect("checkout create should succeed");
    checkouts
        .update_status(name, &created.metadata.resource_version, &CheckoutStatus {
            phase: CheckoutPhase::Ready,
            path: Some(path.to_string()),
            commit: Some("44982740".to_string()),
            message: None,
        })
        .await
        .expect("checkout status update should succeed");
}

async fn create_stopped_terminal(backend: &ResourceBackend, name: &str, env_ref: &str, message: &str) {
    let sessions = backend.clone().using::<flotilla_resources::TerminalSession>(NAMESPACE);
    let created = sessions
        .create(&meta(name), &flotilla_resources::TerminalSessionSpec {
            env_ref: env_ref.to_string(),
            role: "coder".to_string(),
            command: "cargo test".to_string(),
            cwd: "/workspace".to_string(),
            pool: "cleat".to_string(),
        })
        .await
        .expect("terminal create should succeed");
    sessions
        .update_status(name, &created.metadata.resource_version, &TerminalSessionStatus {
            phase: TerminalSessionPhase::Stopped,
            session_id: Some(format!("session-{name}")),
            pid: Some(42),
            started_at: Some(chrono::Utc::now()),
            stopped_at: Some(chrono::Utc::now()),
            inner_command_status: Some(flotilla_resources::InnerCommandStatus::Exited),
            inner_exit_code: Some(1),
            message: Some(message.to_string()),
        })
        .await
        .expect("terminal status update should succeed");
}

fn host_direct_env_name() -> String {
    format!("host-direct-{HOST_REF}")
}
