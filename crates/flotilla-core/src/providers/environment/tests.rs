use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use flotilla_protocol::{DaemonHostPath, EnvironmentSpec, EnvironmentStatus, ImageSource};

use super::{docker::DockerEnvironment, runner::EnvironmentRunner, CreateOpts, EnvironmentProvider};
use crate::providers::{ChannelLabel, CommandOutput, CommandRunner};

/// A mock CommandRunner that records all (cmd, args, cwd) tuples passed to run/run_output.
struct RecordingRunner {
    calls: Mutex<Vec<(String, Vec<String>, PathBuf)>>,
    result: Result<String, String>,
}

impl RecordingRunner {
    fn new_ok(output: &str) -> Self {
        Self { calls: Mutex::new(vec![]), result: Ok(output.to_string()) }
    }

    fn new_err(msg: &str) -> Self {
        Self { calls: Mutex::new(vec![]), result: Err(msg.to_string()) }
    }

    fn calls(&self) -> Vec<(String, Vec<String>, PathBuf)> {
        self.calls.lock().expect("calls mutex").clone()
    }
}

#[async_trait]
impl CommandRunner for RecordingRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path, _label: &ChannelLabel) -> Result<String, String> {
        self.calls.lock().expect("calls mutex").push((cmd.to_string(), args.iter().map(|a| a.to_string()).collect(), cwd.to_path_buf()));
        self.result.clone()
    }

    async fn run_output(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<CommandOutput, String> {
        match self.run(cmd, args, cwd, label).await {
            Ok(stdout) => Ok(CommandOutput { stdout, stderr: String::new(), success: true }),
            Err(stderr) => Ok(CommandOutput { stdout: String::new(), stderr, success: false }),
        }
    }

    async fn exists(&self, _cmd: &str, _args: &[&str]) -> bool {
        true
    }
}

#[tokio::test]
async fn run_wraps_with_docker_exec() {
    let inner = Arc::new(RecordingRunner::new_ok(""));
    let env_runner = EnvironmentRunner::new("test-container".to_string(), inner.clone());
    let label = ChannelLabel::Noop;

    env_runner.run("git", &["status"], Path::new("/workspace"), &label).await.ok();

    let calls = inner.calls();
    assert_eq!(calls.len(), 1);
    let (cmd, args, cwd) = &calls[0];
    assert_eq!(cmd, "docker");
    assert_eq!(args, &["exec", "-w", "/workspace", "test-container", "git", "status"]);
    assert_eq!(cwd, Path::new("/"));
}

#[tokio::test]
async fn run_output_wraps_with_docker_exec() {
    let inner = Arc::new(RecordingRunner::new_ok("output"));
    let env_runner = EnvironmentRunner::new("test-container".to_string(), inner.clone());
    let label = ChannelLabel::Noop;

    env_runner.run_output("git", &["status"], Path::new("/workspace"), &label).await.ok();

    let calls = inner.calls();
    assert_eq!(calls.len(), 1);
    let (cmd, args, cwd) = &calls[0];
    assert_eq!(cmd, "docker");
    assert_eq!(args, &["exec", "-w", "/workspace", "test-container", "git", "status"]);
    assert_eq!(cwd, Path::new("/"));
}

#[tokio::test]
async fn exists_uses_run_with_which() {
    let inner = Arc::new(RecordingRunner::new_ok(""));
    let env_runner = EnvironmentRunner::new("test-container".to_string(), inner.clone());

    let result = env_runner.exists("cleat", &[]).await;

    assert!(result);
    let calls = inner.calls();
    assert_eq!(calls.len(), 1);
    let (cmd, args, cwd) = &calls[0];
    assert_eq!(cmd, "docker");
    assert_eq!(args, &["exec", "test-container", "which", "cleat"]);
    assert_eq!(cwd, Path::new("/"));
}

#[tokio::test]
async fn exists_returns_false_on_failure() {
    let inner = Arc::new(RecordingRunner::new_err("not found"));
    let env_runner = EnvironmentRunner::new("test-container".to_string(), inner.clone());

    let result = env_runner.exists("cleat", &[]).await;

    assert!(!result);
}

// ---------------------------------------------------------------------------
// Multi-response mock runner for sequential command scenarios
// ---------------------------------------------------------------------------

/// A mock CommandRunner that returns successive responses from a queue.
/// Records all calls for later assertion.
struct QueuedRunner {
    calls: Mutex<Vec<(String, Vec<String>, PathBuf)>>,
    responses: Mutex<VecDeque<Result<String, String>>>,
}

impl QueuedRunner {
    fn new(responses: impl IntoIterator<Item = Result<String, String>>) -> Self {
        Self { calls: Mutex::new(vec![]), responses: Mutex::new(responses.into_iter().collect()) }
    }

    fn calls(&self) -> Vec<(String, Vec<String>, PathBuf)> {
        self.calls.lock().expect("calls mutex").clone()
    }
}

#[async_trait]
impl CommandRunner for QueuedRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path, _label: &ChannelLabel) -> Result<String, String> {
        self.calls.lock().expect("calls mutex").push((cmd.to_string(), args.iter().map(|a| a.to_string()).collect(), cwd.to_path_buf()));
        let mut queue = self.responses.lock().expect("responses mutex");
        queue.pop_front().unwrap_or(Err("no more responses".into()))
    }

    async fn run_output(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<CommandOutput, String> {
        match self.run(cmd, args, cwd, label).await {
            Ok(stdout) => Ok(CommandOutput { stdout, stderr: String::new(), success: true }),
            Err(stderr) => Ok(CommandOutput { stdout: String::new(), stderr, success: false }),
        }
    }

    async fn exists(&self, _cmd: &str, _args: &[&str]) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// DockerEnvironment tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ensure_image_builds_dockerfile() {
    let runner = Arc::new(RecordingRunner::new_ok(""));
    let provider = DockerEnvironment::new(runner.clone());
    let spec = EnvironmentSpec { image: ImageSource::Dockerfile("/path/to/Dockerfile".into()), token_requirements: vec![] };

    let result = provider.ensure_image(&spec).await;

    assert!(result.is_ok(), "ensure_image should succeed for Dockerfile source");
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let (cmd, args, _) = &calls[0];
    assert_eq!(cmd, "docker");
    assert_eq!(args[0], "build");
    assert!(args.contains(&"-t".to_string()), "should pass -t flag");
    assert!(args.contains(&"-f".to_string()), "should pass -f flag");
    let f_idx = args.iter().position(|a| a == "-f").expect("-f flag present");
    assert_eq!(args[f_idx + 1], "/path/to/Dockerfile");
}

#[tokio::test]
async fn ensure_image_pulls_registry() {
    let runner = Arc::new(RecordingRunner::new_ok(""));
    let provider = DockerEnvironment::new(runner.clone());
    let spec = EnvironmentSpec { image: ImageSource::Registry("ubuntu:22.04".into()), token_requirements: vec![] };

    let result = provider.ensure_image(&spec).await;

    assert!(result.is_ok(), "ensure_image should succeed for Registry source");
    let image_id = result.unwrap();
    assert_eq!(image_id.as_str(), "ubuntu:22.04");
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let (cmd, args, _) = &calls[0];
    assert_eq!(cmd, "docker");
    assert_eq!(args, &["pull", "ubuntu:22.04"]);
}

#[tokio::test]
async fn create_returns_handle() {
    use flotilla_protocol::ImageId;
    let runner = Arc::new(RecordingRunner::new_ok("container-id-123"));
    let provider = DockerEnvironment::new(runner.clone());
    let image = ImageId::new("ubuntu:22.04");
    let opts = CreateOpts {
        tokens: vec![("GITHUB_TOKEN".into(), "ghp_secret".into())],
        reference_repo: None,
        daemon_socket_path: DaemonHostPath::new("/run/flotilla.sock"),
        working_directory: None,
    };

    let result = provider.create(&image, opts).await;

    assert!(result.is_ok(), "create should succeed");
    let handle = result.unwrap();

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let (cmd, args, _) = &calls[0];
    assert_eq!(cmd, "docker");
    assert_eq!(args[0], "run");
    assert!(args.contains(&"-d".to_string()), "should detach");
    assert!(args.contains(&"--name".to_string()), "should set name");
    assert!(args.contains(&"--label".to_string()), "should set label");
    assert!(args.contains(&"sleep".to_string()), "should run sleep infinity");
    assert!(args.contains(&"infinity".to_string()), "should run sleep infinity");

    // Label should match environment id
    let label_idx = args.iter().position(|a| a == "--label").expect("--label flag");
    let label_val = &args[label_idx + 1];
    assert!(label_val.starts_with("flotilla.environment="), "label should be flotilla.environment=<id>");

    // Environment ID in handle should match label value
    let expected_id = label_val.strip_prefix("flotilla.environment=").unwrap();
    assert_eq!(handle.id().as_str(), expected_id);

    // Token env var should be present
    assert!(args.iter().any(|a| a.starts_with("GITHUB_TOKEN=")), "token env var should be passed");
}

#[tokio::test]
async fn status_returns_running() {
    use flotilla_protocol::ImageId;
    let runner = Arc::new(QueuedRunner::new([
        Ok("container-id".into()), // docker run
        Ok("running".into()),      // docker inspect
    ]));
    let provider = DockerEnvironment::new(runner.clone());
    let image = ImageId::new("ubuntu:22.04");
    let opts = CreateOpts {
        tokens: vec![],
        reference_repo: None,
        daemon_socket_path: DaemonHostPath::new("/run/flotilla.sock"),
        working_directory: None,
    };

    let handle = provider.create(&image, opts).await.expect("create");
    let status = handle.status().await.expect("status");

    assert_eq!(status, EnvironmentStatus::Running);
    let calls = runner.calls();
    // Second call should be docker inspect
    let (cmd, args, _) = &calls[1];
    assert_eq!(cmd, "docker");
    assert_eq!(args[0], "inspect");
    assert!(args.contains(&"--format".to_string()));
}

#[tokio::test]
async fn env_vars_parses_output() {
    use flotilla_protocol::ImageId;
    let runner = Arc::new(QueuedRunner::new([
        Ok("container-id".into()),       // docker run
        Ok("FOO=bar\nBAZ=qux\n".into()), // docker exec sh -lc env
    ]));
    let provider = DockerEnvironment::new(runner.clone());
    let image = ImageId::new("ubuntu:22.04");
    let opts = CreateOpts {
        tokens: vec![],
        reference_repo: None,
        daemon_socket_path: DaemonHostPath::new("/run/flotilla.sock"),
        working_directory: None,
    };

    let handle = provider.create(&image, opts).await.expect("create");
    let vars = handle.env_vars().await.expect("env_vars");

    assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
    assert_eq!(vars.get("BAZ"), Some(&"qux".to_string()));

    let calls = runner.calls();
    let (cmd, args, _) = &calls[1];
    assert_eq!(cmd, "docker");
    assert_eq!(args[0], "exec");
    assert!(args.contains(&"sh".to_string()));
    assert!(args.contains(&"env".to_string()));
}

#[tokio::test]
async fn destroy_calls_docker_rm() {
    use flotilla_protocol::ImageId;
    let runner = Arc::new(QueuedRunner::new([
        Ok("container-id".into()), // docker run
        Ok("".into()),             // docker rm -f
    ]));
    let provider = DockerEnvironment::new(runner.clone());
    let image = ImageId::new("ubuntu:22.04");
    let opts = CreateOpts {
        tokens: vec![],
        reference_repo: None,
        daemon_socket_path: DaemonHostPath::new("/run/flotilla.sock"),
        working_directory: None,
    };

    let handle = provider.create(&image, opts).await.expect("create");
    let container_name = format!("flotilla-env-{}", handle.id());
    handle.destroy().await.expect("destroy");

    let calls = runner.calls();
    let (cmd, args, _) = &calls[1];
    assert_eq!(cmd, "docker");
    assert_eq!(args[0], "rm");
    assert!(args.contains(&"-f".to_string()), "should pass -f flag");
    assert!(args.contains(&container_name), "should pass container name");
}
