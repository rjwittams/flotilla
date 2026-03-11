use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

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

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct HostPath {
    pub host: HostName,
    pub path: PathBuf,
}

impl HostPath {
    pub fn new(host: HostName, path: impl Into<PathBuf>) -> Self {
        Self {
            host,
            path: path.into(),
        }
    }
}

impl fmt::Display for HostPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.path.display())
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepoIdentity {
    pub authority: String,
    pub path: String,
}

impl RepoIdentity {
    /// Extract a RepoIdentity from a git remote URL.
    ///
    /// Handles SSH (`git@github.com:owner/repo.git`) and HTTPS
    /// (`https://github.com/owner/repo.git`). Unknown formats get
    /// authority "unknown" with the full URL as path.
    pub fn from_remote_url(url: &str) -> Option<Self> {
        // SSH format: git@host:owner/repo.git
        if let Some(rest) = url.strip_prefix("git@") {
            if let Some((host, path)) = rest.split_once(':') {
                let path = path.trim_end_matches(".git");
                return Some(Self {
                    authority: host.to_string(),
                    path: path.to_string(),
                });
            }
        }

        // HTTPS/HTTP format: https://host/owner/repo.git
        if url.starts_with("https://") || url.starts_with("http://") {
            if let Ok(parsed) = url::Url::parse(url) {
                if let Some(host) = parsed.host_str() {
                    let path = parsed
                        .path()
                        .trim_start_matches('/')
                        .trim_end_matches(".git");
                    if !path.is_empty() {
                        return Some(Self {
                            authority: host.to_string(),
                            path: path.to_string(),
                        });
                    }
                }
            }
        }

        // SSH shorthand: ssh://git@host/owner/repo.git
        if url.starts_with("ssh://") {
            if let Ok(parsed) = url::Url::parse(url) {
                if let Some(host) = parsed.host_str() {
                    let path = parsed
                        .path()
                        .trim_start_matches('/')
                        .trim_end_matches(".git");
                    if !path.is_empty() {
                        return Some(Self {
                            authority: host.to_string(),
                            path: path.to_string(),
                        });
                    }
                }
            }
        }

        // Unknown format — fallback
        Some(Self {
            authority: "unknown".to_string(),
            path: url.to_string(),
        })
    }
}

impl fmt::Display for RepoIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.authority, self.path)
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

    // HostPath tests

    #[test]
    fn host_path_display_format() {
        let hp = HostPath {
            host: HostName::new("desktop"),
            path: PathBuf::from("/Users/dev/project"),
        };
        assert_eq!(format!("{hp}"), "desktop:/Users/dev/project");
    }

    #[test]
    fn host_path_equality_different_hosts() {
        let a = HostPath {
            host: HostName::new("laptop"),
            path: PathBuf::from("/home/dev/repo"),
        };
        let b = HostPath {
            host: HostName::new("desktop"),
            path: PathBuf::from("/home/dev/repo"),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn host_path_serde_roundtrip() {
        let hp = HostPath {
            host: HostName::new("cloud"),
            path: PathBuf::from("/opt/repos/app"),
        };
        let json = serde_json::to_string(&hp).unwrap();
        let back: HostPath = serde_json::from_str(&json).unwrap();
        assert_eq!(hp, back);
    }

    // RepoIdentity tests

    #[test]
    fn repo_identity_from_github_ssh() {
        let id = RepoIdentity::from_remote_url("git@github.com:rjwittams/flotilla.git");
        assert_eq!(
            id,
            Some(RepoIdentity {
                authority: "github.com".into(),
                path: "rjwittams/flotilla".into()
            })
        );
    }

    #[test]
    fn repo_identity_from_github_https() {
        let id = RepoIdentity::from_remote_url("https://github.com/rjwittams/flotilla.git");
        assert_eq!(
            id,
            Some(RepoIdentity {
                authority: "github.com".into(),
                path: "rjwittams/flotilla".into()
            })
        );
    }

    #[test]
    fn repo_identity_ssh_and_https_match() {
        let ssh = RepoIdentity::from_remote_url("git@github.com:owner/repo.git").unwrap();
        let https = RepoIdentity::from_remote_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(ssh, https);
    }

    #[test]
    fn repo_identity_different_authorities() {
        let gh = RepoIdentity::from_remote_url("git@github.com:team/project.git").unwrap();
        let gl =
            RepoIdentity::from_remote_url("git@gitlab.company.com:team/project.git").unwrap();
        assert_ne!(gh, gl);
    }

    #[test]
    fn repo_identity_unknown_format() {
        let id = RepoIdentity::from_remote_url("file:///local/repo");
        assert_eq!(
            id,
            Some(RepoIdentity {
                authority: "unknown".into(),
                path: "file:///local/repo".into()
            })
        );
    }

    #[test]
    fn repo_identity_display() {
        let id = RepoIdentity {
            authority: "github.com".into(),
            path: "rjwittams/flotilla".into(),
        };
        assert_eq!(format!("{id}"), "github.com:rjwittams/flotilla");
    }
}
