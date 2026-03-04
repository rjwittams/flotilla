pub mod types;
pub mod vcs;
pub mod code_review;
pub mod issue_tracker;
pub mod coding_agent;
pub mod ai_utility;
pub mod workspace;
pub mod registry;
pub mod correlation;
pub mod discovery;

use std::path::Path;

/// Return the user's login shell, defaulting to /bin/zsh.
fn user_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
}

/// Build a single shell command string from a program name and arguments.
/// Arguments containing spaces or special characters are shell-escaped.
fn shell_command_string(cmd: &str, args: &[&str]) -> String {
    let mut parts = vec![cmd.to_string()];
    for arg in args {
        if arg.contains(|c: char| c.is_whitespace() || "\"'\\$`!#&|;(){}[]<>?*~".contains(c)) {
            parts.push(format!("'{}'", arg.replace('\'', "'\\''")));
        } else {
            parts.push(arg.to_string());
        }
    }
    parts.join(" ")
}

/// Shared helper: run a command via the user's login shell and return stdout
/// on success, stderr on failure. Using a login shell ensures the user's PATH,
/// aliases, and shell configuration are available.
pub(crate) async fn run_cmd(cmd: &str, args: &[&str], cwd: &Path) -> Result<String, String> {
    let shell = user_shell();
    let cmd_str = shell_command_string(cmd, args);
    let output = tokio::process::Command::new(&shell)
        .args(["-lc", &cmd_str])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Check if a command exists and runs successfully via the user's interactive
/// login shell. Using `-ic` ensures aliases from .zshrc/.bashrc are available
/// (e.g. `claude` is often installed as a shell alias).
pub(crate) fn command_exists(cmd: &str, args: &[&str]) -> bool {
    let shell = user_shell();
    let cmd_str = shell_command_string(cmd, args);
    std::process::Command::new(&shell)
        .args(["-ic", &cmd_str])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
