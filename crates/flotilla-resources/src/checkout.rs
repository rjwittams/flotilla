use serde::{Deserialize, Serialize};

use crate::{
    resource::{ApiPaths, Resource},
    status_patch::StatusPatch,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkout;

impl Resource for Checkout {
    type Spec = CheckoutSpec;
    type Status = CheckoutStatus;
    type StatusPatch = CheckoutStatusPatch;

    const API_PATHS: ApiPaths = ApiPaths { group: "flotilla.work", version: "v1", plural: "checkouts", kind: "Checkout" };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutSpec {
    pub env_ref: String,
    #[serde(rename = "ref")]
    pub r#ref: String,
    pub target_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<CheckoutWorktreeSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_clone: Option<FreshCloneCheckoutSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutWorktreeSpec {
    pub clone_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreshCloneCheckoutSpec {
    pub url: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckoutPhase {
    #[default]
    Pending,
    Preparing,
    Ready,
    Terminating,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutStatus {
    pub phase: CheckoutPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutStatusPatch {
    MarkPreparing,
    MarkReady { path: String, commit: Option<String> },
    MarkTerminating,
    MarkFailed { message: String },
}

impl StatusPatch<CheckoutStatus> for CheckoutStatusPatch {
    fn apply(&self, status: &mut CheckoutStatus) {
        match self {
            Self::MarkPreparing => {
                status.phase = CheckoutPhase::Preparing;
                status.message = None;
            }
            Self::MarkReady { path, commit } => {
                status.phase = CheckoutPhase::Ready;
                status.path = Some(path.clone());
                status.commit = commit.clone();
                status.message = None;
            }
            Self::MarkTerminating => {
                status.phase = CheckoutPhase::Terminating;
            }
            Self::MarkFailed { message } => {
                status.phase = CheckoutPhase::Failed;
                status.message = Some(message.clone());
            }
        }
    }
}
