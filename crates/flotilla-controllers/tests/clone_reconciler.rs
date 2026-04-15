use std::sync::Arc;

use async_trait::async_trait;
use flotilla_controllers::reconcilers::{CloneReconciler, CloneRuntime};
use flotilla_resources::{clone_key, controller::Reconciler, CloneSpec, ResourceBackend};

mod common;
use common::meta;

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

#[tokio::test]
async fn valid_pending_clone_reconciles_ready() {
    let backend = ResourceBackend::InMemory(Default::default());
    let resolver = backend.using::<flotilla_resources::Clone>("flotilla");
    let canonical = "https://github.com/flotilla-org/flotilla";
    let env_ref = "host-direct-01HXYZ";
    let name = format!("clone-{}", clone_key(canonical, env_ref));
    let clone = resolver
        .create(&meta(&name), &CloneSpec {
            url: "git@github.com:flotilla-org/flotilla.git".to_string(),
            env_ref: env_ref.to_string(),
            path: "/Users/alice/dev/flotilla".to_string(),
        })
        .await
        .expect("create should succeed");
    let reconciler = CloneReconciler::new(Arc::new(FakeCloneRuntime));
    let deps = reconciler.fetch_dependencies(&clone).await.expect("deps should load");
    let outcome = reconciler.reconcile(&clone, &deps, chrono::Utc::now());

    assert!(matches!(
        outcome.patch,
        Some(flotilla_resources::CloneStatusPatch::MarkReady { default_branch: Some(ref branch) }) if branch == "main"
    ));
}

#[tokio::test]
async fn mismatched_clone_name_fails() {
    let backend = ResourceBackend::InMemory(Default::default());
    let resolver = backend.using::<flotilla_resources::Clone>("flotilla");
    let clone = resolver
        .create(&meta("clone-wrong"), &CloneSpec {
            url: "git@github.com:flotilla-org/flotilla.git".to_string(),
            env_ref: "host-direct-01HXYZ".to_string(),
            path: "/Users/alice/dev/flotilla".to_string(),
        })
        .await
        .expect("create should succeed");
    let reconciler = CloneReconciler::new(Arc::new(FakeCloneRuntime));
    let deps = reconciler.fetch_dependencies(&clone).await.expect("deps should load");
    let outcome = reconciler.reconcile(&clone, &deps, chrono::Utc::now());

    assert!(matches!(outcome.patch, Some(flotilla_resources::CloneStatusPatch::MarkFailed { .. })));
}
