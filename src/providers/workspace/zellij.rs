use std::path::PathBuf;
use tokio::process::Command;
use tracing::info;

pub struct ZellijWorkspaceManager;

impl Default for ZellijWorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ZellijWorkspaceManager {
    pub fn new() -> Self {
        Self
    }

    /// Run `zellij action <args>` and return stdout, or an error on failure.
    pub async fn zellij_action(args: &[&str]) -> Result<String, String> {
        let mut cmd_args = vec!["action"];
        cmd_args.extend_from_slice(args);

        let output = Command::new("zellij")
            .args(&cmd_args)
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| format!("failed to run zellij action: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(format!(
                "zellij action {} failed: {}",
                args.first().unwrap_or(&""),
                if stderr.is_empty() { &stdout } else { &stderr }
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Check that `zellij --version` reports >= 0.40.
    /// Parses output like "zellij 0.42.2".
    pub fn check_version() -> Result<(), String> {
        let output = std::process::Command::new("zellij")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("failed to run zellij --version: {e}"))?;

        let version_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let version_part = version_str
            .strip_prefix("zellij ")
            .ok_or_else(|| format!("unexpected zellij version output: {version_str}"))?;

        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() < 2 {
            return Err(format!("cannot parse zellij version: {version_part}"));
        }

        let major: u32 = parts[0]
            .parse()
            .map_err(|_| format!("invalid major version: {}", parts[0]))?;
        let minor: u32 = parts[1]
            .parse()
            .map_err(|_| format!("invalid minor version: {}", parts[1]))?;

        if major == 0 && minor < 40 {
            return Err(format!(
                "zellij >= 0.40 required, found {version_part}"
            ));
        }

        info!("zellij version {version_part} OK");
        Ok(())
    }

    /// Return the current Zellij session name from the environment.
    pub fn session_name() -> Result<String, String> {
        std::env::var("ZELLIJ_SESSION_NAME")
            .map_err(|_| "not running inside a zellij session (ZELLIJ_SESSION_NAME not set)".to_string())
    }

    /// Return the state file path: `~/.config/flotilla/zellij/{session}/state.toml`.
    pub fn state_path(session: &str) -> Result<PathBuf, String> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| "could not determine config directory".to_string())?;
        Ok(config_dir
            .join("flotilla")
            .join("zellij")
            .join(session)
            .join("state.toml"))
    }
}
