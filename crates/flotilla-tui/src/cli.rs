use std::{fmt::Write, path::Path};

use flotilla_core::daemon::DaemonHandle;
use flotilla_protocol::output::OutputFormat;

use crate::socket::SocketDaemon;

pub(crate) fn format_status_human(repos: &[flotilla_protocol::snapshot::RepoInfo]) -> String {
    if repos.is_empty() {
        return "No repos tracked.\n".to_string();
    }
    let mut out = String::new();
    for (i, repo) in repos.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let loading = if repo.loading { "  (loading)" } else { "" };
        writeln!(out, "{}  {}{}", repo.name, repo.path.display(), loading).expect("write to string");
        let health: Vec<String> = repo
            .provider_health
            .iter()
            .flat_map(|(category, providers)| {
                providers.iter().map(move |(name, v)| format!("{category}/{name}: {}", if *v { "ok" } else { "error" }))
            })
            .collect();
        if !health.is_empty() {
            writeln!(out, "  {}", health.join("  ")).expect("write to string");
        }
    }
    out
}

pub(crate) fn format_status_json(repos: &[flotilla_protocol::snapshot::RepoInfo]) -> String {
    #[derive(Debug, serde::Serialize)]
    struct StatusResponse<'a> {
        repos: &'a [flotilla_protocol::snapshot::RepoInfo],
    }
    flotilla_protocol::output::json_pretty(&StatusResponse { repos })
}

pub async fn run_status(socket_path: &Path, format: OutputFormat) -> Result<(), String> {
    let daemon = SocketDaemon::connect(socket_path).await.map_err(|e| format!("cannot connect to daemon: {e}"))?;
    let repos = daemon.list_repos().await.map_err(|e| e.to_string())?;

    let output = match format {
        OutputFormat::Human => format_status_human(&repos),
        OutputFormat::Json => format_status_json(&repos),
    };
    print!("{output}");
    Ok(())
}

pub async fn run_watch(socket_path: &Path, format: OutputFormat) -> Result<(), String> {
    let _ = format;
    let daemon = SocketDaemon::connect(socket_path).await.map_err(|e| format!("cannot connect to daemon: {e}"))?;

    let mut rx = daemon.subscribe();
    println!("watching events (Ctrl-C to stop)...");

    loop {
        match rx.recv().await {
            Ok(event) => {
                let json = serde_json::to_string_pretty(&event).unwrap_or_else(|_| format!("{event:?}"));
                println!("{json}");
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("warning: skipped {n} events");
            }
            Err(_) => {
                eprintln!("daemon disconnected");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf};

    use flotilla_protocol::snapshot::{RepoInfo, RepoLabels};

    fn make_repo(name: &str, path: &str, loading: bool, health: HashMap<String, HashMap<String, bool>>) -> RepoInfo {
        RepoInfo {
            name: name.to_string(),
            path: PathBuf::from(path),
            labels: RepoLabels::default(),
            provider_names: HashMap::new(),
            provider_health: health,
            loading,
        }
    }

    fn health(entries: &[(&str, &str, bool)]) -> HashMap<String, HashMap<String, bool>> {
        let mut map: HashMap<String, HashMap<String, bool>> = HashMap::new();
        for (cat, name, ok) in entries {
            map.entry(cat.to_string()).or_default().insert(name.to_string(), *ok);
        }
        map
    }

    mod status_human {
        use super::*;
        use crate::cli::format_status_human;

        #[test]
        fn empty_repos() {
            assert_eq!(format_status_human(&[]), "No repos tracked.\n");
        }

        #[test]
        fn single_repo_healthy() {
            let repos = vec![make_repo("my-repo", "/tmp/my-repo", false, health(&[("vcs", "Git", true)]))];
            let output = format_status_human(&repos);
            assert!(output.contains("my-repo"), "should contain repo name");
            assert!(output.contains("/tmp/my-repo"), "should contain repo path");
            assert!(output.contains("vcs/Git: ok"), "should show health");
            assert!(!output.contains("loading"), "should not show loading");
        }

        #[test]
        fn repo_loading() {
            let repos = vec![make_repo("my-repo", "/tmp/my-repo", true, HashMap::new())];
            let output = format_status_human(&repos);
            assert!(output.contains("(loading)"), "should show loading indicator");
        }

        #[test]
        fn repo_with_error_health() {
            let repos = vec![make_repo("r", "/tmp/r", false, health(&[("code_review", "GitHub", false)]))];
            let output = format_status_human(&repos);
            assert!(output.contains("code_review/GitHub: error"), "should show error health");
        }
    }

    mod status_json {
        use super::*;
        use crate::cli::format_status_json;

        #[test]
        fn empty_repos_json() {
            let output = format_status_json(&[]);
            let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
            assert_eq!(parsed["repos"], serde_json::json!([]));
        }

        #[test]
        fn repos_wrapped_in_object() {
            let repos = vec![make_repo("my-repo", "/tmp/my-repo", false, HashMap::new())];
            let output = format_status_json(&repos);
            let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
            assert!(parsed["repos"].is_array(), "should have repos array");
            assert_eq!(parsed["repos"][0]["name"], "my-repo");
        }
    }
}
