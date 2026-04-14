use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use flotilla_controllers::reconcilers::{TerminalRuntime, TerminalRuntimeState, TerminalSessionReconciler};
use flotilla_resources::{
    controller::Reconciler, EnvironmentSpec, EnvironmentStatus, EnvironmentStatusPatch, HostDirectEnvironmentSpec, InputMeta,
    ResourceBackend, StatusPatch, TerminalSessionSpec,
};

#[derive(Default)]
struct FakeTerminalRuntime {
    ensured: Mutex<Vec<String>>,
}

#[async_trait]
impl TerminalRuntime for FakeTerminalRuntime {
    async fn ensure_session(&self, name: &str, _spec: &TerminalSessionSpec) -> Result<TerminalRuntimeState, String> {
        self.ensured.lock().expect("ensured lock").push(name.to_string());
        Ok(TerminalRuntimeState { session_id: format!("session-{name}"), pid: Some(42), started_at: Utc::now() })
    }

    async fn kill_session(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

fn meta(name: &str) -> InputMeta {
    InputMeta {
        name: name.to_string(),
        labels: Default::default(),
        annotations: Default::default(),
        owner_references: Vec::new(),
        finalizers: Vec::new(),
        deletion_timestamp: None,
    }
}

#[tokio::test]
async fn terminal_session_waits_for_ready_environment_then_marks_running() {
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
        .update_status("env-a", &env.metadata.resource_version, &EnvironmentStatus::default())
        .await
        .expect("status seed should succeed");
    let current = environments.get("env-a").await.expect("env get should succeed");
    environments
        .update_status("env-a", &current.metadata.resource_version, &{
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
    let runtime = Arc::new(FakeTerminalRuntime::default());
    let reconciler = TerminalSessionReconciler::new(runtime.clone(), backend, "flotilla");
    let deps = reconciler.fetch_dependencies(&session).await.expect("deps should load");
    let outcome = reconciler.reconcile(&session, &deps, Utc::now());

    assert!(matches!(
        outcome.patch,
        Some(flotilla_resources::TerminalSessionStatusPatch::MarkRunning { ref session_id, pid: Some(42), .. })
            if session_id == "session-term-a"
    ));
    assert_eq!(runtime.ensured.lock().expect("ensured lock").as_slice(), &["term-a".to_string()]);
}
