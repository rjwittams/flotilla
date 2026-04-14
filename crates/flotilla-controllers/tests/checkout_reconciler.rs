use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use flotilla_controllers::reconcilers::{CheckoutReconciler, CheckoutRuntime};
use flotilla_resources::{
    controller::Reconciler, CheckoutSpec, CheckoutWorktreeSpec, ClonePhase, CloneSpec, CloneStatus, InputMeta, ResourceBackend,
};

#[derive(Default)]
struct FakeCheckoutRuntime {
    calls: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl CheckoutRuntime for FakeCheckoutRuntime {
    async fn create_worktree(&self, clone_path: &str, branch: &str, target_path: &str) -> Result<Option<String>, String> {
        self.calls.lock().expect("calls lock").push((clone_path.to_string(), branch.to_string(), target_path.to_string()));
        Ok(Some("44982740".to_string()))
    }

    async fn create_fresh_clone(&self, _repo_url: &str, _branch: &str, _target_path: &str) -> Result<Option<String>, String> {
        Ok(Some("44982740".to_string()))
    }

    async fn remove_checkout(&self, _target_path: &str) -> Result<(), String> {
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
async fn worktree_checkout_waits_for_ready_clone_and_then_marks_ready() {
    let backend = ResourceBackend::InMemory(Default::default());
    let clones = backend.clone().using::<flotilla_resources::Clone>("flotilla");
    let checkouts = backend.clone().using::<flotilla_resources::Checkout>("flotilla");
    let clone = clones
        .create(&meta("clone-a"), &CloneSpec {
            url: "git@github.com:flotilla-org/flotilla.git".to_string(),
            env_ref: "host-direct-01HXYZ".to_string(),
            path: "/Users/alice/dev/flotilla".to_string(),
        })
        .await
        .expect("clone create should succeed");
    clones
        .update_status("clone-a", &clone.metadata.resource_version, &CloneStatus {
            phase: ClonePhase::Ready,
            default_branch: Some("main".to_string()),
            message: None,
        })
        .await
        .expect("clone status update should succeed");
    let checkout = checkouts
        .create(&meta("checkout-a"), &CheckoutSpec {
            env_ref: "host-direct-01HXYZ".to_string(),
            r#ref: "feat/convoy-resource".to_string(),
            target_path: "/Users/alice/dev/flotilla.feat-123".to_string(),
            worktree: Some(CheckoutWorktreeSpec { clone_ref: "clone-a".to_string() }),
            fresh_clone: None,
        })
        .await
        .expect("checkout create should succeed");

    let runtime = Arc::new(FakeCheckoutRuntime::default());
    let reconciler = CheckoutReconciler::new(runtime.clone(), backend, "flotilla");
    let deps = reconciler.fetch_dependencies(&checkout).await.expect("deps should load");
    let outcome = reconciler.reconcile(&checkout, &deps, chrono::Utc::now());

    assert!(matches!(
        outcome.patch,
        Some(flotilla_resources::CheckoutStatusPatch::MarkReady { path, commit: Some(ref commit) })
            if path == "/Users/alice/dev/flotilla.feat-123" && commit == "44982740"
    ));
    assert_eq!(runtime.calls.lock().expect("calls lock").as_slice(), &[(
        "/Users/alice/dev/flotilla".to_string(),
        "feat/convoy-resource".to_string(),
        "/Users/alice/dev/flotilla.feat-123".to_string()
    )]);
}
