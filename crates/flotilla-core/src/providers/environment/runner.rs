use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use uuid::Uuid;

use crate::providers::{ChannelLabel, CommandOutput, CommandRunner};

/// A `CommandRunner` decorator that executes all commands inside a Docker container
/// via `docker exec`. The caller's working directory (a path inside the container)
/// is forwarded as a `-w` flag; the host-side cwd is always `/` (irrelevant).
pub struct EnvironmentRunner {
    container_name: String,
    inner: Arc<dyn CommandRunner>,
}

impl EnvironmentRunner {
    pub fn new(container_name: String, inner: Arc<dyn CommandRunner>) -> Self {
        Self { container_name, inner }
    }

    const ENSURE_FILE_IF_ABSENT_SCRIPT: &str = include_str!("../scripts/ensure_file_if_absent.sh");
}

#[async_trait]
impl CommandRunner for EnvironmentRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<String, String> {
        let cwd_str = cwd.to_string_lossy();
        let mut docker_args = vec!["exec", "-w", &cwd_str, &self.container_name, cmd];
        docker_args.extend_from_slice(args);
        self.inner.run("docker", &docker_args, Path::new("/"), label).await
    }

    async fn run_output(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<CommandOutput, String> {
        let cwd_str = cwd.to_string_lossy();
        let mut docker_args = vec!["exec", "-w", &cwd_str, &self.container_name, cmd];
        docker_args.extend_from_slice(args);
        self.inner.run_output("docker", &docker_args, Path::new("/"), label).await
    }

    async fn exists(&self, cmd: &str, _args: &[&str]) -> bool {
        let docker_args = ["exec", &self.container_name, "which", cmd];
        self.inner.run("docker", &docker_args, Path::new("/"), &ChannelLabel::Noop).await.is_ok()
    }

    async fn ensure_file(&self, path: &Path, content: &str) -> Result<String, String> {
        let temp_suffix = Uuid::new_v4().to_string();
        let owned_args = vec![
            "exec".to_string(),
            self.container_name.clone(),
            "sh".to_string(),
            "-lc".to_string(),
            Self::ENSURE_FILE_IF_ABSENT_SCRIPT.to_string(),
            "ensure_file_if_absent.sh".to_string(),
            path.to_string_lossy().into_owned(),
            content.to_owned(),
            temp_suffix,
        ];
        let arg_refs: Vec<&str> = owned_args.iter().map(String::as_str).collect();
        self.inner.run("docker", &arg_refs, Path::new("/"), &ChannelLabel::Noop).await
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use super::EnvironmentRunner;
    use crate::providers::{testing::MockRunner, CommandRunner};

    #[tokio::test]
    async fn ensure_file_delegates_via_docker_exec_sh() {
        let inner = Arc::new(MockRunner::new(vec![Ok(String::new())]));
        let runner = EnvironmentRunner::new("my-container".into(), inner.clone());

        let content = runner.ensure_file(Path::new("/app/config/shpool.toml"), "key = true\n").await.expect("ensure_file");
        assert_eq!(content, String::new());

        let calls = inner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "docker");
        let args = &calls[0].1;
        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"my-container".to_string()));
        assert!(args.contains(&"sh".to_string()));
        assert!(args.contains(&"-lc".to_string()));
        // The sh -c script should create the parent dir and write the file
        let script = args.get(4).expect("should have script arg");
        assert!(script.contains("target=$1"));
        assert_eq!(args.get(6).map(String::as_str), Some("/app/config/shpool.toml"));
        assert_eq!(args.get(7).map(String::as_str), Some("key = true\n"));
    }
}
