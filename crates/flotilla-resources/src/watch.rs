use futures::stream::BoxStream;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    error::ResourceError,
    resource::{Resource, ResourceObject},
};

pub type WatchStream<T> = BoxStream<'static, Result<WatchEvent<T>, ResourceError>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchStart {
    /// Deliver future events only. No replay of current state.
    Now,
    /// Resume from a specific version, delivering all events since that point.
    FromVersion(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T::Spec: Serialize, T::Status: Serialize",
    deserialize = "T::Spec: DeserializeOwned, T::Status: DeserializeOwned"
))]
pub enum WatchEvent<T: Resource> {
    Added(ResourceObject<T>),
    Modified(ResourceObject<T>),
    Deleted(ResourceObject<T>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T::Spec: Serialize, T::Status: Serialize",
    deserialize = "T::Spec: DeserializeOwned, T::Status: DeserializeOwned"
))]
pub struct ResourceList<T: Resource> {
    pub items: Vec<ResourceObject<T>>,
    pub resource_version: String,
}

#[cfg(test)]
mod tests {
    use super::WatchStart;

    #[test]
    fn watch_start_roundtrips_through_serde() {
        let encoded = serde_json::to_string(&WatchStart::FromVersion("7".to_string())).expect("serialize watch start");
        let decoded: WatchStart = serde_json::from_str(&encoded).expect("deserialize watch start");
        assert_eq!(decoded, WatchStart::FromVersion("7".to_string()));
    }
}
