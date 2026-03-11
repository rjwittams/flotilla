use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostName(String);

impl HostName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create a HostName from the local machine's hostname.
    /// Uses `gethostname` crate (already a dependency in flotilla-core).
    pub fn local() -> Self {
        let name = gethostname::gethostname()
            .into_string()
            .unwrap_or_else(|_| "localhost".to_string());
        Self(name)
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_name_display() {
        let h = HostName::new("desktop");
        assert_eq!(h.as_str(), "desktop");
        assert_eq!(format!("{h}"), "desktop");
    }

    #[test]
    fn host_name_equality() {
        let a = HostName::new("desktop");
        let b = HostName::new("desktop");
        let c = HostName::new("laptop");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn host_name_serde_roundtrip() {
        let h = HostName::new("cloud-vm");
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, "\"cloud-vm\"");
        let back: HostName = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back);
    }
}
