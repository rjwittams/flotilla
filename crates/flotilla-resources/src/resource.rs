use std::{collections::BTreeMap, fmt::Debug};

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::status_patch::StatusPatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiPaths {
    pub group: &'static str,
    pub version: &'static str,
    pub plural: &'static str,
    pub kind: &'static str,
}

pub trait Resource: Send + Sync + 'static {
    type Spec: Serialize + DeserializeOwned + Send + Sync + Debug + Clone;
    type Status: Serialize + DeserializeOwned + Send + Sync + Debug + Clone;
    type StatusPatch: StatusPatch<Self::Status>;

    const API_PATHS: ApiPaths;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerReference {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub controller: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, bon::Builder)]
pub struct InputMeta {
    pub name: String,
    #[builder(default)]
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[builder(default)]
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
    #[builder(default)]
    #[serde(default, rename = "ownerReferences", skip_serializing_if = "Vec::is_empty")]
    pub owner_references: Vec<OwnerReference>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finalizers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub name: String,
    pub namespace: String,
    pub resource_version: String,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    #[serde(default, rename = "ownerReferences", skip_serializing_if = "Vec::is_empty")]
    pub owner_references: Vec<OwnerReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finalizers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_timestamp: Option<DateTime<Utc>>,
    pub creation_timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T::Spec: Serialize, T::Status: Serialize",
    deserialize = "T::Spec: DeserializeOwned, T::Status: DeserializeOwned"
))]
pub struct ResourceObject<T: Resource> {
    pub metadata: ObjectMeta,
    pub spec: T::Spec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<T::Status>,
}
