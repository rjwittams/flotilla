use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::info;

/// Manages a shpool daemon subprocess for persistent terminal sessions.
pub struct ShpoolDaemonHandle {
    child: Option<Child>,
    socket_path: PathBuf,
}

impl ShpoolDaemonHandle {
    /// Start a shpool daemon with the given socket path.
    /// If a daemon is already listening on this socket, connects to it instead.
    pub async fn start(socket_path: &Path) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create shpool socket dir: {e}"))?;
        }

        // Check if already running by trying `shpool list`
        let check = Command::new("shpool")
            .args(["--socket", &socket_path.display().to_string(), "list"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        if let Ok(status) = check {
            if status.success() {
                info!("shpool daemon already running at {}", socket_path.display());
                return Ok(Self {
                    child: None,
                    socket_path: socket_path.to_path_buf(),
                });
            }
        }

        info!("starting shpool daemon at {}", socket_path.display());
        let child = Command::new("shpool")
            .args(["--socket", &socket_path.display().to_string(), "daemon"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to start shpool daemon: {e}"))?;

        // Give it a moment to bind the socket
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(Self {
            child: Some(child),
            socket_path: socket_path.to_path_buf(),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for ShpoolDaemonHandle {
    fn drop(&mut self) {
        // Don't kill shpool on drop — sessions should persist across flotilla restarts
        if self.child.is_some() {
            info!("flotilla shutting down, leaving shpool daemon running");
        }
    }
}
