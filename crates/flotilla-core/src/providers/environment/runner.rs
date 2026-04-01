use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use uuid::Uuid;

use crate::providers::{install_managed_helper_script, ChannelLabel, CommandOutput, CommandRunner};

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

    const ENSURE_FILE_IF_ABSENT_NAME: &str = "ensure_file_if_absent.sh";
    const ENSURE_FILE_IF_ABSENT_SCRIPT: &str = include_str!("../scripts/ensure_file_if_absent.sh");

    fn docker_exec_prefix(&self) -> Vec<&str> {
        vec!["exec", &self.container_name]
    }
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
        let helper_path = install_managed_helper_script(
            &*self.inner,
            "docker",
            &self.docker_exec_prefix(),
            Self::ENSURE_FILE_IF_ABSENT_NAME,
            Self::ENSURE_FILE_IF_ABSENT_SCRIPT,
        )
        .await?;
        let mut owned_args: Vec<String> = self.docker_exec_prefix().into_iter().map(str::to_string).collect();
        owned_args.extend([helper_path, path.to_string_lossy().into_owned(), content.to_owned(), temp_suffix]);
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
        let inner = Arc::new(MockRunner::new(vec![Ok(String::new()), Ok(String::new())]));
        let runner = EnvironmentRunner::new("my-container".into(), inner.clone());

        let content = runner.ensure_file(Path::new("/app/config/shpool.toml"), "key = true\n").await.expect("ensure_file");
        assert_eq!(content, String::new());

        let calls = inner.calls();
        assert_eq!(calls.len(), 2);

        assert_eq!(calls[0].0, "docker");
        let install_args = &calls[0].1;
        assert!(install_args.contains(&"exec".to_string()));
        assert!(install_args.contains(&"my-container".to_string()));
        assert!(install_args.contains(&"sh".to_string()));
        assert!(install_args.contains(&"-lc".to_string()));
        let bootstrap_script = install_args.get(4).expect("should have install bootstrap script arg");
        assert!(bootstrap_script.contains("chmod +x"));
        assert_eq!(install_args.get(5).map(String::as_str), Some("flotilla-bootstrap-install-managed-script"));
        assert_eq!(install_args.get(6).map(String::as_str), Some("/tmp/flotilla-tools/ensure_file_if_absent.sh"));

        assert_eq!(calls[1].0, "docker");
        let args = &calls[1].1;
        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"my-container".to_string()));
        assert_eq!(args.get(2).map(String::as_str), Some("/tmp/flotilla-tools/ensure_file_if_absent.sh"));
        assert_eq!(args.get(3).map(String::as_str), Some("/app/config/shpool.toml"));
        assert_eq!(args.get(4).map(String::as_str), Some("key = true\n"));
    }
}
