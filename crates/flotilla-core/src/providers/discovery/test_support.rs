//! Shared mock runner for discovery tests.
//!
//! Provides `DiscoveryMockRunner` — a `CommandRunner` that returns canned
//! responses keyed by `(cmd, args)` and tracks which `cwd` paths and
//! `exists` calls were made.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use async_trait::async_trait;

use crate::providers::{discovery::EnvVars, ChannelLabel, CommandOutput, CommandRunner};

type ResponseMap = HashMap<(String, String), Vec<Result<String, String>>>;

pub struct DiscoveryMockRunnerBuilder {
    responses: ResponseMap,
    tool_exists: HashMap<String, bool>,
}

pub struct DiscoveryMockRunner {
    responses: Mutex<ResponseMap>,
    tool_exists: HashMap<String, bool>,
    seen_cwds: Mutex<Vec<PathBuf>>,
    exists_calls: Mutex<Vec<(String, String)>>,
}

#[derive(Default)]
pub struct TestEnvVars {
    vars: HashMap<String, String>,
}

impl DiscoveryMockRunner {
    pub fn builder() -> DiscoveryMockRunnerBuilder {
        DiscoveryMockRunnerBuilder { responses: HashMap::new(), tool_exists: HashMap::new() }
    }

    #[allow(dead_code)]
    pub fn saw_cwd(&self, cwd: &Path) -> bool {
        self.seen_cwds.lock().expect("lock poisoned").iter().any(|p| p == cwd)
    }

    #[allow(dead_code)]
    pub fn exists_call_count(&self, cmd: &str) -> usize {
        self.exists_calls.lock().expect("lock poisoned").iter().filter(|(called, _)| called == cmd).count()
    }
}

impl DiscoveryMockRunnerBuilder {
    pub fn on_run(mut self, cmd: &str, args: &[&str], response: Result<String, String>) -> Self {
        let key = (cmd.to_string(), args.join(" "));
        self.responses.entry(key).or_default().push(response);
        self
    }

    pub fn tool_exists(mut self, cmd: &str, exists: bool) -> Self {
        self.tool_exists.insert(cmd.to_string(), exists);
        self
    }

    pub fn build(self) -> DiscoveryMockRunner {
        DiscoveryMockRunner {
            responses: Mutex::new(self.responses),
            tool_exists: self.tool_exists,
            seen_cwds: Mutex::new(Vec::new()),
            exists_calls: Mutex::new(Vec::new()),
        }
    }
}

impl TestEnvVars {
    pub fn new<K, V, I>(vars: I) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        I: IntoIterator<Item = (K, V)>,
    {
        Self { vars: vars.into_iter().map(|(key, value)| (key.into(), value.into())).collect() }
    }
}

impl EnvVars for TestEnvVars {
    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
}

#[async_trait]
impl CommandRunner for DiscoveryMockRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path, _label: &ChannelLabel) -> Result<String, String> {
        self.seen_cwds.lock().expect("lock poisoned").push(cwd.to_path_buf());
        let key = (cmd.to_string(), args.join(" "));
        let mut map = self.responses.lock().expect("lock poisoned");
        if let Some(queue) = map.get_mut(&key) {
            if !queue.is_empty() {
                return queue.remove(0);
            }
        }
        Err(format!("DiscoveryMockRunner: no response for {cmd} {}", args.join(" ")))
    }

    async fn run_output(&self, cmd: &str, args: &[&str], cwd: &Path, label: &ChannelLabel) -> Result<CommandOutput, String> {
        match self.run(cmd, args, cwd, label).await {
            Ok(stdout) => Ok(CommandOutput { stdout, stderr: String::new(), success: true }),
            Err(stderr) => Ok(CommandOutput { stdout: String::new(), stderr, success: false }),
        }
    }

    async fn exists(&self, cmd: &str, args: &[&str]) -> bool {
        self.exists_calls.lock().expect("lock poisoned").push((cmd.to_string(), args.join(" ")));
        self.tool_exists.get(cmd).copied().unwrap_or(false)
    }
}

/// Build a `DiscoveryRuntime` that uses no-op env and a minimal fake runner
/// (only responds to `git --version`). Avoids probing ambient host tools.
pub fn fake_discovery(follower: bool) -> super::DiscoveryRuntime {
    minimal_discovery_runtime(
        follower,
        std::sync::Arc::new(DiscoveryMockRunner::builder().on_run("git", &["--version"], Ok("git version 2.43.0".into())).build()),
    )
}

/// Build a `DiscoveryRuntime` that allows real git commands while still
/// avoiding ambient host-tool probes like gh, Codex, Claude, or cmux.
pub fn git_process_discovery(follower: bool) -> super::DiscoveryRuntime {
    minimal_discovery_runtime(follower, std::sync::Arc::new(crate::providers::ProcessCommandRunner))
}

fn minimal_discovery_runtime(follower: bool, runner: std::sync::Arc<dyn CommandRunner>) -> super::DiscoveryRuntime {
    let factories = if follower { super::FactoryRegistry::for_follower() } else { super::FactoryRegistry::default_all() };
    super::DiscoveryRuntime {
        runner,
        env: std::sync::Arc::new(TestEnvVars::default()),
        host_detectors: vec![Box::new(super::detectors::generic::CommandDetector::new(
            "git",
            &["--version"],
            super::detectors::generic::parse_first_dotted_version,
        ))],
        repo_detectors: super::detectors::default_repo_detectors(),
        factories,
    }
}
