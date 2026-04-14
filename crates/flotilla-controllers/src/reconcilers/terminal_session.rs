use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flotilla_resources::{
    controller::{ReconcileOutcome, Reconciler},
    Environment, EnvironmentPhase, ResourceBackend, ResourceError, ResourceObject, TerminalSession, TerminalSessionPhase,
    TerminalSessionStatusPatch, TypedResolver,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRuntimeState {
    pub session_id: String,
    pub pid: Option<i64>,
    pub started_at: DateTime<Utc>,
}

#[async_trait]
pub trait TerminalRuntime: Send + Sync {
    async fn ensure_session(&self, name: &str, spec: &flotilla_resources::TerminalSessionSpec) -> Result<TerminalRuntimeState, String>;
    async fn kill_session(&self, session_id: &str) -> Result<(), String>;
}

pub struct TerminalSessionReconciler<R> {
    runtime: Arc<R>,
    environments: TypedResolver<Environment>,
}

impl<R> TerminalSessionReconciler<R> {
    pub fn new(runtime: Arc<R>, backend: ResourceBackend, namespace: &str) -> Self {
        Self { runtime, environments: backend.using::<Environment>(namespace) }
    }
}

pub enum TerminalDeps {
    None,
    Waiting,
    Running(TerminalRuntimeState),
    Failed(String),
}

impl<R> Reconciler for TerminalSessionReconciler<R>
where
    R: TerminalRuntime + 'static,
{
    type Resource = TerminalSession;
    type Dependencies = TerminalDeps;

    async fn fetch_dependencies(&self, obj: &ResourceObject<Self::Resource>) -> Result<Self::Dependencies, ResourceError> {
        if obj.status.as_ref().map(|status| status.phase).unwrap_or(TerminalSessionPhase::Starting) != TerminalSessionPhase::Starting {
            return Ok(TerminalDeps::None);
        }

        let environment = match self.environments.get(&obj.spec.env_ref).await {
            Ok(environment) => environment,
            Err(ResourceError::NotFound { .. }) => return Ok(TerminalDeps::Waiting),
            Err(err) => return Err(err),
        };
        if environment.status.as_ref().map(|status| status.phase) != Some(EnvironmentPhase::Ready) {
            return Ok(TerminalDeps::Waiting);
        }

        Ok(match self.runtime.ensure_session(&obj.metadata.name, &obj.spec).await {
            Ok(state) => TerminalDeps::Running(state),
            Err(err) => TerminalDeps::Failed(err),
        })
    }

    fn reconcile(
        &self,
        obj: &ResourceObject<Self::Resource>,
        deps: &Self::Dependencies,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> ReconcileOutcome<Self::Resource> {
        let patch =
            if obj.status.as_ref().map(|status| status.phase).unwrap_or(TerminalSessionPhase::Starting) == TerminalSessionPhase::Starting {
                match deps {
                    TerminalDeps::Running(state) => Some(TerminalSessionStatusPatch::MarkRunning {
                        session_id: state.session_id.clone(),
                        pid: state.pid,
                        started_at: state.started_at,
                    }),
                    TerminalDeps::Failed(message) => {
                        Some(TerminalSessionStatusPatch::MarkFailed { message: message.clone(), stopped_at: Some(Utc::now()) })
                    }
                    TerminalDeps::Waiting | TerminalDeps::None => None,
                }
            } else {
                None
            };

        ReconcileOutcome { patch, actuations: Vec::new(), events: Vec::new(), requeue_after: None }
    }

    async fn run_finalizer(&self, obj: &ResourceObject<Self::Resource>) -> Result<(), ResourceError> {
        if let Some(session_id) = obj.status.as_ref().and_then(|status| status.session_id.as_deref()) {
            self.runtime.kill_session(session_id).await.map_err(ResourceError::other)?;
        }
        Ok(())
    }

    fn finalizer_name(&self) -> Option<&'static str> {
        Some("flotilla.work/terminal-teardown")
    }
}
