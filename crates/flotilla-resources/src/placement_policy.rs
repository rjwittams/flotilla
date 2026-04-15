use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    resource::{ApiPaths, Resource},
    status_patch::NoStatusPatch,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementPolicy;

impl Resource for PlacementPolicy {
    type Spec = PlacementPolicySpec;
    type Status = ();
    type StatusPatch = NoStatusPatch;

    const API_PATHS: ApiPaths = ApiPaths { group: "flotilla.work", version: "v1", plural: "placementpolicies", kind: "PlacementPolicy" };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
pub struct PlacementPolicySpec {
    pub pool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_direct: Option<HostDirectPlacementPolicySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker_per_task: Option<DockerPerTaskPlacementPolicySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDirectPlacementPolicySpec {
    pub host_ref: String,
    pub checkout: HostDirectPlacementPolicyCheckout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostDirectPlacementPolicyCheckout {
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerPerTaskPlacementPolicySpec {
    pub host_ref: String,
    pub image: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub checkout: DockerCheckoutStrategy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockerCheckoutStrategy {
    WorktreeOnHostAndMount { mount_path: String },
    FreshCloneInContainer { clone_path: String },
}
