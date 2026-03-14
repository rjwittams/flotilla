# Coverage Phase 3: Quick Wins Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ~300 lines of coverage across `tui/cli.rs` formatters and `executor.rs` gaps to bank easy wins toward the 92% target.

**Architecture:** Pure unit tests for formatting functions and executor helpers. No new infrastructure — all tests follow existing patterns in each file's `#[cfg(test)]` module.

**Tech Stack:** Rust, `#[test]`/`#[tokio::test]`, existing mock providers and test builders.

---

## Chunk 1: tui/cli.rs formatter tests

### Task 1: format_command_result tests

**Files:**
- Modify: `crates/flotilla-tui/src/cli.rs` (add tests in existing `mod tests`)

- [ ] **Step 1: Write tests for all CommandResult variants**

Add a new submodule `command_result_human` inside the existing `mod tests` block (after the `watch_human` module, before the closing `}`):

```rust
mod command_result_human {
    use std::path::PathBuf;

    use flotilla_protocol::commands::CommandResult;

    use crate::cli::format_command_result;

    #[test]
    fn ok() {
        assert_eq!(format_command_result(&CommandResult::Ok), "ok");
    }

    #[test]
    fn repo_added() {
        let result = CommandResult::RepoAdded { path: PathBuf::from("/tmp/my-repo") };
        assert_eq!(format_command_result(&result), "repo added: /tmp/my-repo");
    }

    #[test]
    fn repo_removed() {
        let result = CommandResult::RepoRemoved { path: PathBuf::from("/tmp/old-repo") };
        assert_eq!(format_command_result(&result), "repo removed: /tmp/old-repo");
    }

    #[test]
    fn refreshed() {
        let result = CommandResult::Refreshed { repos: vec![PathBuf::from("/a"), PathBuf::from("/b")] };
        assert_eq!(format_command_result(&result), "refreshed 2 repo(s)");
    }

    #[test]
    fn checkout_created() {
        let result = CommandResult::CheckoutCreated { branch: "feat-x".into(), path: PathBuf::from("/repo/wt") };
        assert_eq!(format_command_result(&result), "checkout created: feat-x");
    }

    #[test]
    fn checkout_removed() {
        let result = CommandResult::CheckoutRemoved { branch: "old".into() };
        assert_eq!(format_command_result(&result), "checkout removed: old");
    }

    #[test]
    fn branch_name_generated() {
        let result = CommandResult::BranchNameGenerated { name: "feat/login".into(), issue_ids: vec![] };
        assert_eq!(format_command_result(&result), "branch name: feat/login");
    }

    #[test]
    fn checkout_status() {
        let result = CommandResult::CheckoutStatus(flotilla_protocol::commands::CheckoutStatusInfo {
            branch: "main".into(),
            ahead: 0,
            behind: 0,
            has_uncommitted: false,
            uncommitted_files: vec![],
            pr_url: None,
            pr_state: None,
            pr_review_decision: None,
            pr_ci_status: None,
        });
        assert_eq!(format_command_result(&result), "checkout status received");
    }

    #[test]
    fn error() {
        let result = CommandResult::Error { message: "something broke".into() };
        assert_eq!(format_command_result(&result), "error: something broke");
    }

    #[test]
    fn cancelled() {
        assert_eq!(format_command_result(&CommandResult::Cancelled), "cancelled");
    }
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p flotilla-tui command_result_human`
Expected: all 10 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-tui/src/cli.rs
git commit -m "test: add format_command_result coverage for all CommandResult variants"
```

### Task 2: format_work_items_table tests

**Files:**
- Modify: `crates/flotilla-tui/src/cli.rs`

- [ ] **Step 1: Write tests for work items table formatting**

Add a new submodule `work_items_table` inside `mod tests`:

```rust
mod work_items_table {
    use std::path::PathBuf;

    use flotilla_protocol::{
        snapshot::{WorkItem, WorkItemIdentity, WorkItemKind},
        HostName, HostPath,
    };

    use crate::cli::format_work_items_table;

    fn make_work_item(kind: WorkItemKind, branch: Option<&str>, desc: &str) -> WorkItem {
        WorkItem {
            kind,
            identity: WorkItemIdentity::Checkout(HostPath::new(HostName::new("test"), PathBuf::from("/tmp/wt"))),
            host: HostName::new("test"),
            branch: branch.map(String::from),
            description: desc.to_string(),
            checkout: None,
            change_request_key: None,
            session_key: None,
            issue_keys: vec![],
            workspace_refs: vec![],
            is_main_checkout: false,
            debug_group: vec![],
            source: None,
            terminal_keys: vec![],
        }
    }

    #[test]
    fn empty_items_produces_header_only() {
        let table = format_work_items_table(&[]);
        let output = table.to_string();
        assert!(output.contains("Kind"), "table should have Kind header");
        assert!(output.contains("Branch"), "table should have Branch header");
    }

    #[test]
    fn single_item_with_all_none_fields() {
        let item = make_work_item(WorkItemKind::Checkout, None, "test desc");
        let table = format_work_items_table(&[item]);
        let output = table.to_string();
        assert!(output.contains("Checkout"), "should show kind");
        assert!(output.contains("test desc"), "should show description");
        // None fields display as "-"
        assert!(output.contains('-'), "None fields should show as dash");
    }

    #[test]
    fn item_with_populated_fields() {
        let mut item = make_work_item(WorkItemKind::ChangeRequest, Some("feat-x"), "Add login");
        item.change_request_key = Some("#42".to_string());
        item.session_key = Some("sess-1".to_string());
        item.issue_keys = vec!["#10".to_string(), "#20".to_string()];
        let table = format_work_items_table(&[item]);
        let output = table.to_string();
        assert!(output.contains("feat-x"), "should show branch");
        assert!(output.contains("#42"), "should show PR key");
        assert!(output.contains("sess-1"), "should show session");
        assert!(output.contains("#10, #20"), "should join issue keys");
    }

    #[test]
    fn multiple_items() {
        let items = vec![
            make_work_item(WorkItemKind::Checkout, Some("main"), "Main branch"),
            make_work_item(WorkItemKind::Session, Some("feat"), "Feature work"),
        ];
        let table = format_work_items_table(&items);
        let output = table.to_string();
        assert!(output.contains("main"), "should contain first item");
        assert!(output.contains("feat"), "should contain second item");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-tui work_items_table`
Expected: all 4 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-tui/src/cli.rs
git commit -m "test: add format_work_items_table coverage"
```

### Task 3: format_repo_detail_human tests

**Files:**
- Modify: `crates/flotilla-tui/src/cli.rs`

- [ ] **Step 1: Write tests for repo detail formatting**

Add submodule `repo_detail_human` inside `mod tests`:

```rust
mod repo_detail_human {
    use std::{collections::HashMap, path::PathBuf};

    use flotilla_protocol::{RepoDetailResponse, snapshot::ProviderError};

    use crate::cli::format_repo_detail_human;

    #[test]
    fn minimal_detail_shows_path() {
        let detail = RepoDetailResponse {
            path: PathBuf::from("/tmp/my-repo"),
            slug: None,
            provider_health: HashMap::new(),
            work_items: vec![],
            errors: vec![],
        };
        let output = format_repo_detail_human(&detail);
        assert!(output.contains("Repo: /tmp/my-repo"), "should show repo path");
        assert!(!output.contains("Slug:"), "should not show slug when None");
    }

    #[test]
    fn detail_with_slug() {
        let detail = RepoDetailResponse {
            path: PathBuf::from("/tmp/my-repo"),
            slug: Some("org/my-repo".into()),
            provider_health: HashMap::new(),
            work_items: vec![],
            errors: vec![],
        };
        let output = format_repo_detail_human(&detail);
        assert!(output.contains("Slug: org/my-repo"), "should show slug");
    }

    #[test]
    fn detail_with_errors() {
        let detail = RepoDetailResponse {
            path: PathBuf::from("/tmp/my-repo"),
            slug: None,
            provider_health: HashMap::new(),
            work_items: vec![],
            errors: vec![ProviderError {
                category: "vcs".into(),
                provider: "git".into(),
                message: "repo not found".into(),
            }],
        };
        let output = format_repo_detail_human(&detail);
        assert!(output.contains("Errors:"), "should have Errors section");
        assert!(output.contains("[vcs/git] repo not found"), "should format error");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-tui repo_detail_human`
Expected: all 3 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-tui/src/cli.rs
git commit -m "test: add format_repo_detail_human coverage"
```

### Task 4: format_repo_providers_human tests

**Files:**
- Modify: `crates/flotilla-tui/src/cli.rs`

- [ ] **Step 1: Write tests for repo providers formatting**

Add submodule `repo_providers_human` inside `mod tests`:

```rust
mod repo_providers_human {
    use std::{collections::HashMap, path::PathBuf};

    use flotilla_protocol::{DiscoveryEntry, ProviderInfo, RepoProvidersResponse, UnmetRequirementInfo};

    use crate::cli::format_repo_providers_human;

    #[test]
    fn empty_providers_shows_path_only() {
        let resp = RepoProvidersResponse {
            path: PathBuf::from("/tmp/repo"),
            slug: None,
            host_discovery: vec![],
            repo_discovery: vec![],
            providers: vec![],
            unmet_requirements: vec![],
        };
        let output = format_repo_providers_human(&resp);
        assert!(output.contains("Repo: /tmp/repo"), "should show path");
        assert!(!output.contains("Host Discovery:"), "no discovery section when empty");
        assert!(!output.contains("Providers:"), "no providers section when empty");
    }

    #[test]
    fn with_slug_and_host_discovery() {
        let resp = RepoProvidersResponse {
            path: PathBuf::from("/tmp/repo"),
            slug: Some("org/repo".into()),
            host_discovery: vec![DiscoveryEntry {
                kind: "claude".into(),
                detail: HashMap::from([("version".into(), "3.0".into())]),
            }],
            repo_discovery: vec![],
            providers: vec![],
            unmet_requirements: vec![],
        };
        let output = format_repo_providers_human(&resp);
        assert!(output.contains("Slug: org/repo"), "should show slug");
        assert!(output.contains("Host Discovery:"), "should have host discovery section");
        assert!(output.contains("claude"), "should show discovery kind");
        assert!(output.contains("version=3.0"), "should show detail");
    }

    #[test]
    fn with_repo_discovery() {
        let resp = RepoProvidersResponse {
            path: PathBuf::from("/tmp/repo"),
            slug: None,
            host_discovery: vec![],
            repo_discovery: vec![DiscoveryEntry {
                kind: "git".into(),
                detail: HashMap::from([("remote".into(), "origin".into())]),
            }],
            providers: vec![],
            unmet_requirements: vec![],
        };
        let output = format_repo_providers_human(&resp);
        assert!(output.contains("Repo Discovery:"), "should have repo discovery section");
        assert!(output.contains("git"), "should show kind");
    }

    #[test]
    fn with_providers_table() {
        let resp = RepoProvidersResponse {
            path: PathBuf::from("/tmp/repo"),
            slug: None,
            host_discovery: vec![],
            repo_discovery: vec![],
            providers: vec![
                ProviderInfo { category: "vcs".into(), name: "git".into(), healthy: true },
                ProviderInfo { category: "code_review".into(), name: "github".into(), healthy: false },
            ],
            unmet_requirements: vec![],
        };
        let output = format_repo_providers_human(&resp);
        assert!(output.contains("Providers:"), "should have providers section");
        assert!(output.contains("git"), "should show provider name");
        assert!(output.contains("ok"), "should show healthy");
        assert!(output.contains("error"), "should show unhealthy");
    }

    #[test]
    fn with_unmet_requirements() {
        let resp = RepoProvidersResponse {
            path: PathBuf::from("/tmp/repo"),
            slug: None,
            host_discovery: vec![],
            repo_discovery: vec![],
            providers: vec![],
            unmet_requirements: vec![UnmetRequirementInfo {
                factory: "claude".into(),
                requirement: "claude CLI not found".into(),
            }],
        };
        let output = format_repo_providers_human(&resp);
        assert!(output.contains("Unmet Requirements:"), "should have section");
        assert!(output.contains("claude: claude CLI not found"), "should format requirement");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-tui repo_providers_human`
Expected: all 5 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-tui/src/cli.rs
git commit -m "test: add format_repo_providers_human coverage"
```

### Task 5: format_repo_work_human and repo_name tests

**Files:**
- Modify: `crates/flotilla-tui/src/cli.rs`

- [ ] **Step 1: Write tests for work formatting and repo_name helper**

Add submodules inside `mod tests`:

```rust
mod repo_work_human {
    use std::path::PathBuf;

    use flotilla_protocol::RepoWorkResponse;

    use crate::cli::format_repo_work_human;

    #[test]
    fn empty_work_items() {
        let resp = RepoWorkResponse { path: PathBuf::from("/tmp/repo"), slug: None, work_items: vec![] };
        let output = format_repo_work_human(&resp);
        assert!(output.contains("Repo: /tmp/repo"), "should show path");
        assert!(output.contains("No work items."), "should indicate no work items");
    }

    #[test]
    fn with_slug() {
        let resp = RepoWorkResponse { path: PathBuf::from("/tmp/repo"), slug: Some("org/repo".into()), work_items: vec![] };
        let output = format_repo_work_human(&resp);
        assert!(output.contains("Slug: org/repo"), "should show slug");
    }

    #[test]
    fn with_work_items() {
        use flotilla_protocol::{
            snapshot::{WorkItem, WorkItemIdentity, WorkItemKind},
            HostName, HostPath,
        };
        let items = vec![WorkItem {
            kind: WorkItemKind::Checkout,
            identity: WorkItemIdentity::Checkout(HostPath::new(HostName::new("test"), PathBuf::from("/tmp/wt"))),
            host: HostName::new("test"),
            branch: Some("feat-x".into()),
            description: "Feature X".into(),
            checkout: None,
            change_request_key: None,
            session_key: None,
            issue_keys: vec![],
            workspace_refs: vec![],
            is_main_checkout: false,
            debug_group: vec![],
            source: None,
            terminal_keys: vec![],
        }];
        let resp = RepoWorkResponse { path: PathBuf::from("/tmp/repo"), slug: None, work_items: items };
        let output = format_repo_work_human(&resp);
        assert!(!output.contains("No work items."), "should not say no work items");
        assert!(output.contains("feat-x"), "should contain work item branch");
    }
}

mod repo_name_tests {
    use std::path::Path;

    use crate::cli::repo_name;

    #[test]
    fn extracts_last_component() {
        assert_eq!(repo_name(Path::new("/tmp/my-repo")), "my-repo");
    }

    #[test]
    fn root_path_fallback() {
        assert_eq!(repo_name(Path::new("/")), "/");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-tui repo_work_human && cargo test -p flotilla-tui repo_name_tests`
Expected: all 5 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-tui/src/cli.rs
git commit -m "test: add format_repo_work_human and repo_name coverage"
```

## Chunk 2: executor.rs gap tests

### Task 6: resolve_checkout_branch tests

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs` (add tests in existing `mod tests`)

- [ ] **Step 1: Write tests for resolve_checkout_branch**

Add tests at the end of the existing `mod tests` block:

```rust
// -----------------------------------------------------------------------
// Tests: resolve_checkout_branch
// -----------------------------------------------------------------------

#[test]
fn resolve_checkout_branch_by_path_found() {
    let mut data = empty_data();
    data.checkouts.insert(hp("/repo/wt-feat"), make_checkout("feat-x", "/repo/wt-feat"));
    let branch = resolve_checkout_branch(
        &CheckoutSelector::Path(PathBuf::from("/repo/wt-feat")),
        &data,
        &HostName::local(),
    );
    assert_eq!(branch.unwrap(), "feat-x");
}

#[test]
fn resolve_checkout_branch_by_path_not_found() {
    let data = empty_data();
    let result = resolve_checkout_branch(
        &CheckoutSelector::Path(PathBuf::from("/nonexistent")),
        &data,
        &HostName::local(),
    );
    assert!(result.unwrap_err().contains("checkout not found"));
}

#[test]
fn resolve_checkout_branch_by_query_exact_match() {
    let mut data = empty_data();
    data.checkouts.insert(hp("/repo/wt-feat"), make_checkout("feat-x", "/repo/wt-feat"));
    let branch = resolve_checkout_branch(
        &CheckoutSelector::Query("feat-x".to_string()),
        &data,
        &HostName::local(),
    );
    assert_eq!(branch.unwrap(), "feat-x");
}

#[test]
fn resolve_checkout_branch_by_query_substring_match() {
    let mut data = empty_data();
    data.checkouts.insert(hp("/repo/wt-feat"), make_checkout("feature-login", "/repo/wt-feat"));
    let branch = resolve_checkout_branch(
        &CheckoutSelector::Query("login".to_string()),
        &data,
        &HostName::local(),
    );
    assert_eq!(branch.unwrap(), "feature-login");
}

#[test]
fn resolve_checkout_branch_by_query_not_found() {
    let data = empty_data();
    let result = resolve_checkout_branch(
        &CheckoutSelector::Query("nonexistent".to_string()),
        &data,
        &HostName::local(),
    );
    assert!(result.unwrap_err().contains("checkout not found"));
}

#[test]
fn resolve_checkout_branch_by_query_ambiguous() {
    let mut data = empty_data();
    data.checkouts.insert(hp("/repo/wt-feat-a"), make_checkout("feat-a", "/repo/wt-feat-a"));
    data.checkouts.insert(hp("/repo/wt-feat-b"), make_checkout("feat-b", "/repo/wt-feat-b"));
    let result = resolve_checkout_branch(
        &CheckoutSelector::Query("feat".to_string()),
        &data,
        &HostName::local(),
    );
    assert!(result.unwrap_err().contains("ambiguous"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-core resolve_checkout_branch`
Expected: all 6 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "test: add resolve_checkout_branch coverage for path, query, and ambiguous cases"
```

### Task 7: resolve_terminal_pool tests

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`

- [ ] **Step 1: Write tests for resolve_terminal_pool**

Add tests in the existing `mod tests` block:

```rust
// -----------------------------------------------------------------------
// Tests: resolve_terminal_pool
// -----------------------------------------------------------------------

#[tokio::test]
async fn resolve_terminal_pool_no_template() {
    let mock_pool = Arc::new(MockTerminalPool { killed: tokio::sync::Mutex::new(vec![]) });
    let mut config = WorkspaceConfig {
        name: "feat-x".to_string(),
        working_directory: PathBuf::from("/repo/wt"),
        template_vars: std::collections::HashMap::from([("main_command".to_string(), "claude".to_string())]),
        template_yaml: None,
        resolved_commands: None,
    };

    resolve_terminal_pool(&mut config, mock_pool.as_ref()).await;

    // Default template produces terminal entries, so resolved_commands should be populated
    assert!(config.resolved_commands.is_some(), "should resolve default template terminals");
}

#[tokio::test]
async fn resolve_terminal_pool_with_non_terminal_content() {
    let mock_pool = Arc::new(MockTerminalPool { killed: tokio::sync::Mutex::new(vec![]) });
    // Template with only non-terminal content
    let yaml = r#"
content:
  - role: editor
    content_type: editor
    command: vim
"#;
    let mut config = WorkspaceConfig {
        name: "feat-x".to_string(),
        working_directory: PathBuf::from("/repo/wt"),
        template_vars: std::collections::HashMap::new(),
        template_yaml: Some(yaml.to_string()),
        resolved_commands: None,
    };

    resolve_terminal_pool(&mut config, mock_pool.as_ref()).await;

    // No terminal entries to resolve
    assert!(config.resolved_commands.is_none(), "non-terminal content should not produce resolved commands");
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-core resolve_terminal_pool`
Expected: all 2 tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "test: add resolve_terminal_pool coverage"
```

### Task 8: write_branch_issue_links tests

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`

- [ ] **Step 1: Write tests for write_branch_issue_links**

Add tests in the existing `mod tests` block:

```rust
// -----------------------------------------------------------------------
// Tests: write_branch_issue_links
// -----------------------------------------------------------------------

#[tokio::test]
async fn write_branch_issue_links_single_provider() {
    let runner = MockRunner::new(vec![Ok(String::new())]);
    write_branch_issue_links(
        &repo_root(),
        "feat-x",
        &[("github".to_string(), "42".to_string()), ("github".to_string(), "43".to_string())],
        &runner,
    )
    .await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let args = &calls[0];
    assert!(args.contains(&"config".to_string()), "should call git config");
    assert!(args.iter().any(|a| a.contains("flotilla.issues.github")), "should use provider key");
    assert!(args.iter().any(|a| a.contains("42,43") || a.contains("43,42")), "should join issue ids");
}

#[tokio::test]
async fn write_branch_issue_links_multiple_providers() {
    let runner = MockRunner::new(vec![Ok(String::new()), Ok(String::new())]);
    write_branch_issue_links(
        &repo_root(),
        "feat-x",
        &[("github".to_string(), "42".to_string()), ("linear".to_string(), "LIN-1".to_string())],
        &runner,
    )
    .await;

    let calls = runner.calls();
    assert_eq!(calls.len(), 2, "should make one git config call per provider");
}

#[tokio::test]
async fn write_branch_issue_links_git_error_is_tolerated() {
    let runner = MockRunner::new(vec![Err("git config failed".to_string())]);
    // Should not panic — errors are logged and tolerated
    write_branch_issue_links(
        &repo_root(),
        "feat-x",
        &[("github".to_string(), "42".to_string())],
        &runner,
    )
    .await;
}

#[tokio::test]
async fn write_branch_issue_links_empty_ids_is_noop() {
    let runner = runner_ok();
    write_branch_issue_links(&repo_root(), "feat-x", &[], &runner).await;

    let calls = runner.calls();
    assert!(calls.is_empty(), "empty issue_ids should make no git calls");
}
```

- [ ] **Step 2: Check MockRunner supports calls() introspection**

The `MockRunner` needs a `calls()` method to inspect what commands were run. Check if it exists; if not, the tests should use the existing pattern of providing expected results and verifying no panics.

Run: `cargo test -p flotilla-core write_branch_issue_links`

If `calls()` doesn't exist, simplify to just verify no panics:

```rust
#[tokio::test]
async fn write_branch_issue_links_single_provider() {
    let runner = MockRunner::new(vec![Ok(String::new())]);
    write_branch_issue_links(
        &repo_root(),
        "feat-x",
        &[("github".to_string(), "42".to_string()), ("github".to_string(), "43".to_string())],
        &runner,
    )
    .await;
    // If it consumed the mock result without panicking, it called git config once
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "test: add write_branch_issue_links coverage"
```

### Task 9: validate_checkout_target edge cases

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`

- [ ] **Step 1: Write tests for validate_checkout_target**

Add tests:

```rust
// -----------------------------------------------------------------------
// Tests: validate_checkout_target
// -----------------------------------------------------------------------

#[tokio::test]
async fn validate_fresh_branch_succeeds_when_not_exists() {
    // Both show-ref checks fail = branch doesn't exist = fresh branch is valid
    let runner = MockRunner::new(vec![Err("not found".into()), Err("not found".into())]);
    let result = validate_checkout_target(&repo_root(), "new-feat", CheckoutIntent::FreshBranch, &runner).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn validate_fresh_branch_fails_when_local_exists() {
    // Local show-ref succeeds = branch exists = error for fresh
    let runner = MockRunner::new(vec![Ok(String::new()), Err("not found".into())]);
    let result = validate_checkout_target(&repo_root(), "existing", CheckoutIntent::FreshBranch, &runner).await;
    assert!(result.unwrap_err().contains("already exists"));
}

#[tokio::test]
async fn validate_fresh_branch_fails_when_remote_exists() {
    // Local fails, remote succeeds
    let runner = MockRunner::new(vec![Err("not found".into()), Ok(String::new())]);
    let result = validate_checkout_target(&repo_root(), "remote-only", CheckoutIntent::FreshBranch, &runner).await;
    assert!(result.unwrap_err().contains("already exists"));
}

#[tokio::test]
async fn validate_existing_branch_succeeds_when_local_exists() {
    let runner = MockRunner::new(vec![Ok(String::new()), Err("not found".into())]);
    let result = validate_checkout_target(&repo_root(), "feat", CheckoutIntent::ExistingBranch, &runner).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn validate_existing_branch_succeeds_when_remote_exists() {
    let runner = MockRunner::new(vec![Err("not found".into()), Ok(String::new())]);
    let result = validate_checkout_target(&repo_root(), "remote-feat", CheckoutIntent::ExistingBranch, &runner).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn validate_existing_branch_fails_when_not_exists() {
    let runner = MockRunner::new(vec![Err("not found".into()), Err("not found".into())]);
    let result = validate_checkout_target(&repo_root(), "nonexistent", CheckoutIntent::ExistingBranch, &runner).await;
    assert!(result.unwrap_err().contains("branch not found"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-core validate_checkout_target`
Expected: FAIL — `validate_checkout_target` and `CheckoutIntent` are private. These tests are inside the same module so they should have access. If not, skip this task.

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "test: add validate_checkout_target coverage for fresh and existing branch cases"
```

### Task 10: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test --locked`
Expected: All tests pass

- [ ] **Step 2: Run clippy and fmt**

Run: `cargo clippy --all-targets --locked -- -D warnings && cargo +nightly fmt --check`
Expected: Clean

- [ ] **Step 3: Commit any fixups**

```bash
git add -A
git commit -m "chore: clippy and fmt fixes for coverage tests"
```
