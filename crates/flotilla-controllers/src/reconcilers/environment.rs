use std::sync::Arc;

use async_trait::async_trait;
use flotilla_resources::{
    controller::{ReconcileOutcome, Reconciler},
    DockerEnvironmentSpec, Environment, EnvironmentPhase, EnvironmentStatusPatch, ResourceError, ResourceObject,
};

#[async_trait]
pub trait DockerEnvironmentRuntime: Send + Sync {
    async fn provision(&self, name: &str, spec: &DockerEnvironmentSpec) -> Result<String, String>;
    async fn destroy(&self, container_id: &str) -> Result<(), String>;
}

pub struct EnvironmentReconciler<R> {
    docker: Arc<R>,
}

impl<R> EnvironmentReconciler<R> {
    pub fn new(docker: Arc<R>) -> Self {
        Self { docker }
    }
}

pub enum EnvironmentDeps {
    None,
    Ready { docker_container_id: Option<String> },
    Failed(String),
}

impl<R> Reconciler for EnvironmentReconciler<R>
where
    R: DockerEnvironmentRuntime + 'static,
{
    type Resource = Environment;
    type Dependencies = EnvironmentDeps;

    async fn fetch_dependencies(&self, obj: &ResourceObject<Self::Resource>) -> Result<Self::Dependencies, ResourceError> {
        match obj.status.as_ref().map(|status| status.phase).unwrap_or(EnvironmentPhase::Pending) {
            EnvironmentPhase::Pending => {
                if let Some(spec) = &obj.spec.docker {
                    match self.docker.provision(&obj.metadata.name, spec).await {
                        Ok(container_id) => Ok(EnvironmentDeps::Ready { docker_container_id: Some(container_id) }),
                        Err(err) => Ok(EnvironmentDeps::Failed(err)),
                    }
                } else {
                    Ok(EnvironmentDeps::None)
                }
            }
            _ => Ok(EnvironmentDeps::None),
        }
    }

    fn reconcile(
        &self,
        obj: &ResourceObject<Self::Resource>,
        deps: &Self::Dependencies,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> ReconcileOutcome<Self::Resource> {
        let patch = match obj.status.as_ref().map(|status| status.phase).unwrap_or(EnvironmentPhase::Pending) {
            EnvironmentPhase::Pending if obj.spec.host_direct.is_some() => {
                Some(EnvironmentStatusPatch::MarkReady { docker_container_id: None })
            }
            EnvironmentPhase::Pending => match deps {
                EnvironmentDeps::Ready { docker_container_id } => {
                    Some(EnvironmentStatusPatch::MarkReady { docker_container_id: docker_container_id.clone() })
                }
                EnvironmentDeps::Failed(message) => Some(EnvironmentStatusPatch::MarkFailed { message: message.clone() }),
                EnvironmentDeps::None => None,
            },
            _ => None,
        };

        ReconcileOutcome { patch, actuations: Vec::new(), events: Vec::new(), requeue_after: None }
    }

    async fn run_finalizer(&self, obj: &ResourceObject<Self::Resource>) -> Result<(), ResourceError> {
        if let Some(container_id) = obj.status.as_ref().and_then(|status| status.docker_container_id.as_deref()) {
            self.docker.destroy(container_id).await.map_err(ResourceError::other)?;
        }
        Ok(())
    }

    fn finalizer_name(&self) -> Option<&'static str> {
        Some("flotilla.work/environment-teardown")
    }
}
