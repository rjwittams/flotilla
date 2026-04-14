use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use flotilla_controllers::reconcilers::{DockerEnvironmentRuntime, EnvironmentReconciler};
use flotilla_resources::{
    controller::Reconciler, DockerEnvironmentSpec, EnvironmentMount, EnvironmentMountMode, EnvironmentSpec, HostDirectEnvironmentSpec,
    InputMeta, ResourceBackend,
};

#[derive(Default)]
struct FakeDockerRuntime {
    destroyed: Mutex<Vec<String>>,
}

#[async_trait]
impl DockerEnvironmentRuntime for FakeDockerRuntime {
    async fn provision(&self, name: &str, _spec: &DockerEnvironmentSpec) -> Result<String, String> {
        Ok(format!("container-{name}"))
    }

    async fn destroy(&self, container_id: &str) -> Result<(), String> {
        self.destroyed.lock().expect("destroyed lock").push(container_id.to_string());
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
async fn host_direct_environment_reconciles_ready_without_runtime() {
    let backend = ResourceBackend::InMemory(Default::default());
    let resolver = backend.using::<flotilla_resources::Environment>("flotilla");
    let env = resolver
        .create(&meta("host-direct-01HXYZ"), &EnvironmentSpec {
            host_direct: Some(HostDirectEnvironmentSpec {
                host_ref: "01HXYZ".to_string(),
                repo_default_dir: "/Users/alice/dev/flotilla-repos".to_string(),
            }),
            docker: None,
        })
        .await
        .expect("create should succeed");
    let reconciler = EnvironmentReconciler::new(Arc::new(FakeDockerRuntime::default()));
    let deps = reconciler.fetch_dependencies(&env).await.expect("deps should load");
    let outcome = reconciler.reconcile(&env, &deps, chrono::Utc::now());

    assert!(matches!(outcome.patch, Some(flotilla_resources::EnvironmentStatusPatch::MarkReady { docker_container_id: None })));
}

#[tokio::test]
async fn docker_environment_reconciles_ready_with_container_id() {
    let backend = ResourceBackend::InMemory(Default::default());
    let resolver = backend.using::<flotilla_resources::Environment>("flotilla");
    let env = resolver
        .create(&meta("docker-env"), &EnvironmentSpec {
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
        .expect("create should succeed");
    let reconciler = EnvironmentReconciler::new(Arc::new(FakeDockerRuntime::default()));
    let deps = reconciler.fetch_dependencies(&env).await.expect("deps should load");
    let outcome = reconciler.reconcile(&env, &deps, chrono::Utc::now());

    assert!(matches!(
        outcome.patch,
        Some(flotilla_resources::EnvironmentStatusPatch::MarkReady { docker_container_id: Some(ref id) }) if id == "container-docker-env"
    ));
}
