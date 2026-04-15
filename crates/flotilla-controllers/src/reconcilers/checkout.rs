use std::sync::Arc;

use async_trait::async_trait;
use flotilla_resources::{
    controller::{ReconcileOutcome, Reconciler},
    Checkout, CheckoutPhase, CheckoutStatusPatch, Clone, ClonePhase, ResourceBackend, ResourceError, ResourceObject, TypedResolver,
};

#[async_trait]
pub trait CheckoutRuntime: Send + Sync {
    async fn create_worktree(&self, clone_path: &str, branch: &str, target_path: &str) -> Result<Option<String>, String>;
    async fn create_fresh_clone(&self, repo_url: &str, branch: &str, target_path: &str) -> Result<Option<String>, String>;
    async fn remove_checkout(&self, target_path: &str) -> Result<(), String>;
}

pub struct CheckoutReconciler<R> {
    runtime: Arc<R>,
    clones: TypedResolver<Clone>,
}

impl<R> CheckoutReconciler<R> {
    pub fn new(runtime: Arc<R>, backend: ResourceBackend, namespace: &str) -> Self {
        Self { runtime, clones: backend.using::<Clone>(namespace) }
    }
}

pub enum CheckoutDeps {
    None,
    Ready { commit: Option<String> },
    Waiting,
    Failed(String),
}

impl<R> Reconciler for CheckoutReconciler<R>
where
    R: CheckoutRuntime + 'static,
{
    type Resource = Checkout;
    type Dependencies = CheckoutDeps;

    async fn fetch_dependencies(&self, obj: &ResourceObject<Self::Resource>) -> Result<Self::Dependencies, ResourceError> {
        if obj.status.as_ref().map(|status| status.phase).unwrap_or(CheckoutPhase::Pending) != CheckoutPhase::Pending {
            return Ok(CheckoutDeps::None);
        }

        if let Some(worktree) = &obj.spec.worktree {
            let clone = match self.clones.get(&worktree.clone_ref).await {
                Ok(clone) => clone,
                Err(ResourceError::NotFound { .. }) => return Ok(CheckoutDeps::Waiting),
                Err(err) => return Err(err),
            };
            if clone.status.as_ref().map(|status| status.phase) != Some(ClonePhase::Ready) {
                return Ok(CheckoutDeps::Waiting);
            }
            if clone.spec.env_ref != obj.spec.env_ref {
                return Ok(CheckoutDeps::Failed("worktree clone env_ref mismatch".to_string()));
            }
            return Ok(match self.runtime.create_worktree(&clone.spec.path, &obj.spec.r#ref, &obj.spec.target_path).await {
                Ok(commit) => CheckoutDeps::Ready { commit },
                Err(err) => CheckoutDeps::Failed(err),
            });
        }

        if let Some(fresh_clone) = &obj.spec.fresh_clone {
            return Ok(match self.runtime.create_fresh_clone(&fresh_clone.url, &obj.spec.r#ref, &obj.spec.target_path).await {
                Ok(commit) => CheckoutDeps::Ready { commit },
                Err(err) => CheckoutDeps::Failed(err),
            });
        }

        Ok(CheckoutDeps::Failed("checkout spec missing strategy".to_string()))
    }

    fn reconcile(
        &self,
        obj: &ResourceObject<Self::Resource>,
        deps: &Self::Dependencies,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> ReconcileOutcome<Self::Resource> {
        let patch = if obj.status.as_ref().map(|status| status.phase).unwrap_or(CheckoutPhase::Pending) == CheckoutPhase::Pending {
            match deps {
                CheckoutDeps::Ready { commit } => {
                    Some(CheckoutStatusPatch::MarkReady { path: obj.spec.target_path.clone(), commit: commit.clone() })
                }
                CheckoutDeps::Failed(message) => Some(CheckoutStatusPatch::MarkFailed { message: message.clone() }),
                CheckoutDeps::Waiting | CheckoutDeps::None => None,
            }
        } else {
            None
        };

        ReconcileOutcome::new(patch)
    }

    async fn run_finalizer(&self, obj: &ResourceObject<Self::Resource>) -> Result<(), ResourceError> {
        self.runtime.remove_checkout(&obj.spec.target_path).await.map_err(ResourceError::other)
    }

    fn finalizer_name(&self) -> Option<&'static str> {
        Some("flotilla.work/checkout-cleanup")
    }
}
