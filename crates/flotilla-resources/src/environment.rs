use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    resource::{ApiPaths, Resource},
    status_patch::StatusPatch,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Environment;

impl Resource for Environment {
    type Spec = EnvironmentSpec;
    type Status = EnvironmentStatus;
    type StatusPatch = EnvironmentStatusPatch;

    const API_PATHS: ApiPaths = ApiPaths { group: "flotilla.work", version: "v1", plural: "environments", kind: "Environment" };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_direct: Option<HostDirectEnvironmentSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerEnvironmentSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDirectEnvironmentSpec {
    pub host_ref: String,
    pub repo_default_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerEnvironmentSpec {
    pub host_ref: String,
    pub image: String,
    #[serde(default)]
    pub mounts: Vec<EnvironmentMount>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentMount {
    pub source_path: String,
    pub target_path: String,
    pub mode: EnvironmentMountMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentMountMode {
    Ro,
    Rw,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvironmentPhase {
    #[default]
    Pending,
    Ready,
    Terminating,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentStatus {
    pub phase: EnvironmentPhase,
    #[serde(default)]
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker_container_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentStatusPatch {
    MarkReady { docker_container_id: Option<String> },
    MarkFailed { message: String },
    MarkTerminating,
}

impl StatusPatch<EnvironmentStatus> for EnvironmentStatusPatch {
    fn apply(&self, status: &mut EnvironmentStatus) {
        match self {
            Self::MarkReady { docker_container_id } => {
                status.phase = EnvironmentPhase::Ready;
                status.ready = true;
                status.docker_container_id = docker_container_id.clone();
                status.message = None;
            }
            Self::MarkFailed { message } => {
                status.phase = EnvironmentPhase::Failed;
                status.ready = false;
                status.message = Some(message.clone());
            }
            Self::MarkTerminating => {
                status.phase = EnvironmentPhase::Terminating;
                status.ready = false;
            }
        }
    }
}
