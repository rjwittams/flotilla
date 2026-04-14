use serde::{Deserialize, Serialize};

use crate::{
    resource::{ApiPaths, Resource},
    status_patch::StatusPatch,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clone;

impl Resource for Clone {
    type Spec = CloneSpec;
    type Status = CloneStatus;
    type StatusPatch = CloneStatusPatch;

    const API_PATHS: ApiPaths = ApiPaths { group: "flotilla.work", version: "v1", plural: "clones", kind: "Clone" };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloneSpec {
    pub url: String,
    pub env_ref: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClonePhase {
    #[default]
    Pending,
    Cloning,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloneStatus {
    pub phase: ClonePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneStatusPatch {
    MarkCloning,
    MarkReady { default_branch: Option<String> },
    MarkFailed { message: String },
}

impl StatusPatch<CloneStatus> for CloneStatusPatch {
    fn apply(&self, status: &mut CloneStatus) {
        match self {
            Self::MarkCloning => {
                status.phase = ClonePhase::Cloning;
                status.message = None;
            }
            Self::MarkReady { default_branch } => {
                status.phase = ClonePhase::Ready;
                status.default_branch = default_branch.clone();
                status.message = None;
            }
            Self::MarkFailed { message } => {
                status.phase = ClonePhase::Failed;
                status.message = Some(message.clone());
            }
        }
    }
}
