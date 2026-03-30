use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use flotilla_protocol::arg::shell_quote;

use crate::providers::{ChannelLabel, CommandOutput, CommandRunner};

/// Command runner that executes commands on a remote host over SSH.
///
/// This is intentionally narrow: it shells out to `ssh` for direct-environment
/// execution and discovery, but it does not model daemon-to-daemon transport.
pub struct SshCommandRunner {
    destination: String,
    multiplex: bool,
    runner: Arc<dyn CommandRunner>,
}

impl SshCommandRunner {
    pub fn new(destination: impl Into<String>, multiplex: bool, runner: Arc<dyn CommandRunner>) -> Self {
        Self { destination: destination.into(), multiplex, runner }
    }

    fn ssh_args<'a>(&'a self, script: &'a str) -> Vec<&'a str> {
        let mut args = vec!["-T", "-o", "BatchMode=yes"];
        if self.multiplex {
            args.extend(["-o", "ControlMaster=auto", "-o", "ControlPersist=60"]);
        }
        args.push(self.destination.as_str());
        args.push("sh");
        args.push("-lc");
        args.push(script);
        args
    }

    fn remote_script(&self, cmd: &str, args: &[&str], cwd: &Path) -> String {
        let mut parts = Vec::with_capacity(args.len() + 4);
        parts.push(format!("cd {}", shell_quote(&cwd.to_string_lossy())));
        parts.push("&&".to_string());
        parts.push("exec".to_string());
        parts.push(shell_quote(cmd));
        parts.extend(args.iter().map(|arg| shell_quote(arg)));
        parts.join(" ")
    }

    async fn execute(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<String, String> {
        let script = self.remote_script(cmd, args, cwd);
        let ssh_args = self.ssh_args(&script);
        self.runner.run("ssh", &ssh_args, Path::new("/"), label).await
    }
}

#[async_trait]
impl CommandRunner for SshCommandRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<String, String> {
        self.execute(cmd, args, cwd, label).await
    }

    async fn run_output(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<CommandOutput, String> {
        let script = self.remote_script(cmd, args, cwd);
        let ssh_args = self.ssh_args(&script);
        self.runner.run_output("ssh", &ssh_args, Path::new("/"), label).await
    }

    async fn exists(&self, cmd: &str, _args: &[&str]) -> bool {
        let script = format!("command -v {} >/dev/null 2>&1", shell_quote(cmd));
        let ssh_args = self.ssh_args(&script);
        self.runner.run("ssh", &ssh_args, Path::new("/"), &ChannelLabel::Noop).await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use async_trait::async_trait;

    use super::SshCommandRunner;
    use crate::providers::{ChannelLabel, CommandOutput, CommandRunner};

    struct RecordingRunner {
        calls: Mutex<Vec<(String, Vec<String>, PathBuf)>>,
        run_result: Mutex<Option<Result<String, String>>>,
        run_output_result: Mutex<Option<Result<CommandOutput, String>>>,
    }

    impl RecordingRunner {
        fn with_run_result(result: Result<String, String>) -> Self {
            Self { calls: Mutex::new(Vec::new()), run_result: Mutex::new(Some(result)), run_output_result: Mutex::new(None) }
        }

        fn with_run_output_result(result: Result<CommandOutput, String>) -> Self {
            Self { calls: Mutex::new(Vec::new()), run_result: Mutex::new(None), run_output_result: Mutex::new(Some(result)) }
        }

        fn calls(&self) -> Vec<(String, Vec<String>, PathBuf)> {
            self.calls.lock().expect("calls mutex").clone()
        }
    }

    #[async_trait]
    impl CommandRunner for RecordingRunner {
        async fn run(&self, cmd: &str, args: &[&str], cwd: &Path, _label: &ChannelLabel) -> Result<String, String> {
            self.calls.lock().expect("calls mutex").push((
                cmd.to_string(),
                args.iter().map(|arg| (*arg).to_string()).collect(),
                cwd.to_path_buf(),
            ));
            self.run_result.lock().expect("run_result mutex").take().expect("run result not configured")
        }

        async fn run_output(&self, cmd: &str, args: &[&str], cwd: &Path, _label: &ChannelLabel) -> Result<CommandOutput, String> {
            self.calls.lock().expect("calls mutex").push((
                cmd.to_string(),
                args.iter().map(|arg| (*arg).to_string()).collect(),
                cwd.to_path_buf(),
            ));
            self.run_output_result.lock().expect("run_output_result mutex").take().expect("run output result not configured")
        }

        async fn exists(&self, _cmd: &str, _args: &[&str]) -> bool {
            true
        }
    }

    fn ssh_call_args(calls: &[(String, Vec<String>, PathBuf)]) -> &Vec<String> {
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "ssh");
        &calls[0].1
    }

    #[tokio::test]
    async fn run_builds_ssh_command_with_working_directory() {
        let inner = std::sync::Arc::new(RecordingRunner::with_run_result(Ok("stdout".into())));
        let runner = SshCommandRunner::new("alice@feta.local", false, inner.clone());

        let output = runner.run("git", &["status", "--short"], Path::new("/repo with space"), &ChannelLabel::Noop).await;

        assert_eq!(output.unwrap(), "stdout");
        let calls = inner.calls();
        let args = ssh_call_args(&calls);
        assert_eq!(args[0], "-T");
        assert_eq!(args[1], "-o");
        assert_eq!(args[2], "BatchMode=yes");
        assert_eq!(args[3], "alice@feta.local");
        assert_eq!(args[4], "sh");
        assert_eq!(args[5], "-lc");
        assert_eq!(args[6], "cd '/repo with space' && exec 'git' 'status' '--short'");
    }

    #[tokio::test]
    async fn run_output_preserves_stdout_and_stderr() {
        let inner = std::sync::Arc::new(RecordingRunner::with_run_output_result(Ok(CommandOutput {
            stdout: "out".into(),
            stderr: "err".into(),
            success: false,
        })));
        let runner = SshCommandRunner::new("alice@feta.local", true, inner.clone());

        let output = runner.run_output("git", &["status"], Path::new("/repo"), &ChannelLabel::Noop).await.unwrap();

        assert_eq!(output.stdout, "out");
        assert_eq!(output.stderr, "err");
        assert!(!output.success);

        let calls = inner.calls();
        let args = ssh_call_args(&calls);
        assert_eq!(args[0], "-T");
        assert_eq!(args[1], "-o");
        assert_eq!(args[2], "BatchMode=yes");
        assert_eq!(&args[3..7], ["-o", "ControlMaster=auto", "-o", "ControlPersist=60"]);
        assert_eq!(args[7], "alice@feta.local");
        assert_eq!(args[8], "sh");
        assert_eq!(args[9], "-lc");
        assert_eq!(args[10], "cd '/repo' && exec 'git' 'status'");
    }

    #[tokio::test]
    async fn exists_uses_remote_command_lookup() {
        let inner = std::sync::Arc::new(RecordingRunner::with_run_result(Ok(String::new())));
        let runner = SshCommandRunner::new("alice@feta.local", false, inner.clone());

        assert!(runner.exists("cleat", &[]).await);

        let calls = inner.calls();
        let args = ssh_call_args(&calls);
        assert_eq!(args[0], "-T");
        assert_eq!(args[1], "-o");
        assert_eq!(args[2], "BatchMode=yes");
        assert_eq!(args[3], "alice@feta.local");
        assert_eq!(args[4], "sh");
        assert_eq!(args[5], "-lc");
        assert_eq!(args[6], "command -v 'cleat' >/dev/null 2>&1");
    }

    #[tokio::test]
    async fn run_propagates_runner_errors() {
        let inner = std::sync::Arc::new(RecordingRunner::with_run_result(Err("ssh failed".into())));
        let runner = SshCommandRunner::new("alice@feta.local", false, inner.clone());

        let error = runner.run("git", &["status"], Path::new("/repo"), &ChannelLabel::Noop).await;

        assert_eq!(error.unwrap_err(), "ssh failed");
    }
}
