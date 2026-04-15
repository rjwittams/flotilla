use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use flotilla_controllers::reconcilers::{TerminalRuntime, TerminalRuntimeState, TerminalSessionReconciler};
use flotilla_resources::{
    controller::Reconciler, EnvironmentSpec, EnvironmentStatus, EnvironmentStatusPatch, HostDirectEnvironmentSpec, ResourceBackend,
    StatusPatch, TerminalSessionSpec,
};

mod common;
use common::meta;

#[tokio::test]
async fn terminal_session_failure_uses_injected_now_for_stopped_at() {
    let backend = ResourceBackend::InMemory(Default::default());
    let environments = backend.clone().using::<flotilla_resources::Environment>("flotilla");
    let sessions = backend.clone().using::<flotilla_resources::TerminalSession>("flotilla");
    let env = environments
        .create(&meta("env-a"), &EnvironmentSpec {
            host_direct: Some(HostDirectEnvironmentSpec {
                host_ref: "01HXYZ".to_string(),
                repo_default_dir: "/Users/alice/dev/flotilla-repos".to_string(),
            }),
            docker: None,
        })
        .await
        .expect("env create should succeed");
    environments
        .update_status("env-a", &env.metadata.resource_version, &{
            let mut status = EnvironmentStatus::default();
            EnvironmentStatusPatch::MarkReady { docker_container_id: None }.apply(&mut status);
            status
        })
        .await
        .expect("env ready update should succeed");

    let session = sessions
        .create(&meta("term-a"), &TerminalSessionSpec {
            env_ref: "env-a".to_string(),
            role: "coder".to_string(),
            command: "cargo test".to_string(),
            cwd: "/workspace".to_string(),
            pool: "cleat".to_string(),
        })
        .await
        .expect("session create should succeed");
    let reconciler = TerminalSessionReconciler::new(Arc::new(FailingTerminalRuntime), backend, "flotilla");
    let deps = reconciler.fetch_dependencies(&session).await.expect("deps should load");
    let now = Utc::now();
    let outcome = reconciler.reconcile(&session, &deps, now);

    assert!(matches!(
        outcome.patch,
        Some(flotilla_resources::TerminalSessionStatusPatch::MarkFailed { stopped_at: Some(stopped_at), .. })
            if stopped_at == now
    ));
}

struct FailingTerminalRuntime;

#[async_trait]
impl TerminalRuntime for FailingTerminalRuntime {
    async fn ensure_session(&self, _name: &str, _spec: &TerminalSessionSpec) -> Result<TerminalRuntimeState, String> {
        Err("boom".to_string())
    }

    async fn kill_session(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}
