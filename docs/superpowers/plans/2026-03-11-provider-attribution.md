# Provider Attribution Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface provider identity throughout the UI — in the table (new Source column, Agt abbreviation), preview panel (provider-prefixed titles + session ID), event log (provider field), and protocol types.

**Architecture:** Add `provider_name: String` to protocol data types (`CloudAgentSession`, `ChangeRequest`, `Issue`), add `source: Option<String>` to protocol `WorkItem`, thread provider identity from providers through correlation into the UI. The correlation engine itself is not changed — provider names are read from the data structs at conversion time.

**Tech Stack:** Rust, flotilla-protocol (serde types), flotilla-core (correlation/conversion), flotilla-tui (ratatui rendering), tracing (structured logging)

**Spec:** `docs/superpowers/specs/2026-03-11-provider-attribution-design.md`

---

## Chunk 1: Protocol + Core Type Changes (single atomic commit)

### Task 1: Add `provider_name` to protocol data types and `source` to WorkItem

All struct changes and construction-site fixes happen together so the workspace always compiles.

**Files:**
- Modify: `crates/flotilla-protocol/src/provider_data.rs` (structs + tests)
- Modify: `crates/flotilla-protocol/src/snapshot.rs` (WorkItem + tests)
- Modify: All `flotilla-core` files that construct `CloudAgentSession`, `ChangeRequest`, `Issue`, or `WorkItem`

- [ ] **Step 1: Add `provider_name` to `CloudAgentSession`**

In `crates/flotilla-protocol/src/provider_data.rs` at line 98:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudAgentSession {
    pub title: String,
    pub status: SessionStatus,
    pub model: Option<String>,
    pub updated_at: Option<String>,
    pub correlation_keys: Vec<CorrelationKey>,
    #[serde(default)]
    pub provider_name: String,
}
```

- [ ] **Step 2: Add `provider_name` to `ChangeRequest`**

At line 55:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRequest {
    pub title: String,
    pub branch: String,
    pub status: ChangeRequestStatus,
    pub body: Option<String>,
    pub correlation_keys: Vec<CorrelationKey>,
    pub association_keys: Vec<AssociationKey>,
    #[serde(default)]
    pub provider_name: String,
}
```

- [ ] **Step 3: Add `provider_name` to `Issue`**

At line 73:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub title: String,
    pub labels: Vec<String>,
    pub association_keys: Vec<AssociationKey>,
    #[serde(default)]
    pub provider_name: String,
}
```

- [ ] **Step 4: Add `source` to `WorkItem`**

In `crates/flotilla-protocol/src/snapshot.rs` at line 80:

```rust
pub struct WorkItem {
    pub kind: WorkItemKind,
    pub identity: WorkItemIdentity,
    pub branch: Option<String>,
    pub description: String,
    pub checkout: Option<CheckoutRef>,
    pub change_request_key: Option<String>,
    pub session_key: Option<String>,
    pub issue_keys: Vec<String>,
    pub workspace_refs: Vec<String>,
    pub is_main_checkout: bool,
    #[serde(default)]
    pub debug_group: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
}
```

- [ ] **Step 5: Set `provider_name` in provider implementations**

In **`coding_agent/claude.rs`** (line 386 session construction):
```rust
CloudAgentSession {
    title: s.title,
    status,
    model,
    updated_at: Some(s.updated_at.clone()),
    correlation_keys,
    provider_name: "claude".into(),
}
```

In **`coding_agent/codex.rs`** (session construction):
Add `provider_name: "codex".into()` to `CloudAgentSession { ... }`.

In **`coding_agent/cursor.rs`** (session construction):
Add `provider_name: "cursor".into()` to `CloudAgentSession { ... }`.

In **`code_review/github.rs`** (line 100 CR construction):
```rust
ChangeRequest {
    title: pr.title.clone(),
    branch: pr.head_ref_name.clone(),
    status,
    body: pr.body.clone(),
    correlation_keys,
    association_keys,
    provider_name: self.provider_name.clone(),
}
```

In **`issue_tracker/github.rs`** (line 49 `parse_issue`):
```rust
Issue {
    title,
    labels,
    association_keys,
    provider_name: provider_name.to_string(),
}
```

- [ ] **Step 6: Fix ALL remaining construction sites across the workspace**

Run `cargo build --workspace 2>&1` and fix every compilation error. Known sites needing `provider_name`:

**`CloudAgentSession` in flotilla-core:**
- `data.rs` test helpers (around lines 838, 1892, 1902, 1912)
- `delta.rs` test helper (around line 176)
- `refresh.rs` test helper (around line 672)
- `executor.rs` test helper (around line 804)
- `claude.rs`, `codex.rs`, `cursor.rs` test code

**`ChangeRequest` in flotilla-core:**
- `data.rs` test helper (around line 819)
- `delta.rs` test helper (around line 157)
- `refresh.rs` test helper (around line 661)
- `in_process.rs` test helper (around line 1131)
- `code_review/github.rs` test code

**`Issue` in flotilla-core:**
- `data.rs` test helper (around line 848)
- `delta.rs` test helper (around line 168)
- `executor.rs` test helper (around line 817)
- `issue_cache.rs` test helpers (around lines 131, 212)
- `in_process.rs` test code (around lines 1166, 1175, 1272)
- `issue_tracker/github.rs` test code

**`WorkItem` in flotilla-protocol + flotilla-core:**
- `snapshot.rs` tests (lines 236-264, 291-335) — add `source: None`
- `convert.rs` tests (lines 118-151, 154-172) — add `source: None`

For all test helpers and test code, use `provider_name: String::new()` (or `"test-provider".into()` where it aids readability). For `WorkItem`, use `source: None`.

- [ ] **Step 7: Verify full workspace compiles and tests pass**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: add provider_name to protocol data types and source to WorkItem"
```

## Chunk 2: Core Conversion + Abbreviation Rename

### Task 2: Add `gethostname` dependency

- [ ] **Step 1: Add dependency**

```bash
cargo add gethostname -p flotilla-core
```

- [ ] **Step 2: Commit**

```bash
git add crates/flotilla-core/Cargo.toml Cargo.lock
git commit -m "chore: add gethostname dependency to flotilla-core"
```

### Task 3: Populate `WorkItem.source` during conversion

**Files:**
- Modify: `crates/flotilla-core/src/data.rs:56-65` (`CorrelatedWorkItem`), `data.rs:68-71` (`StandaloneResult`), `data.rs:237-330` (`group_to_work_item`), `data.rs:464-472` (standalone issues)
- Modify: `crates/flotilla-core/src/convert.rs:15-46` (`correlation_result_to_work_item`)
- Test: `crates/flotilla-core/src/convert.rs` (new + existing tests)

- [ ] **Step 1: Write failing tests for `source` population**

In `crates/flotilla-core/src/convert.rs`, add tests:

```rust
#[test]
fn convert_correlated_checkout_has_hostname_source() {
    let hostname = gethostname::gethostname().to_string_lossy().into_owned();
    let item = CorrelationResult::Correlated(CorrelatedWorkItem {
        anchor: CorrelatedAnchor::Checkout(CheckoutRef {
            key: PathBuf::from("/repos/proj/wt"),
            is_main_checkout: false,
        }),
        branch: Some("feat".to_string()),
        description: "Feature".to_string(),
        linked_change_request: None,
        linked_session: None,
        linked_issues: vec![],
        workspace_refs: vec![],
        correlation_group_idx: 0,
        source: Some(hostname.clone()),
    });

    let proto = correlation_result_to_work_item(&item, &[]);
    assert_eq!(proto.source, Some(hostname));
}

#[test]
fn convert_correlated_session_has_provider_source() {
    let item = CorrelationResult::Correlated(CorrelatedWorkItem {
        anchor: CorrelatedAnchor::Session("sess-1".to_string()),
        branch: None,
        description: "My session".to_string(),
        linked_change_request: None,
        linked_session: None,
        linked_issues: vec![],
        workspace_refs: vec![],
        correlation_group_idx: 0,
        source: Some("claude".to_string()),
    });

    let proto = correlation_result_to_work_item(&item, &[]);
    assert_eq!(proto.source, Some("claude".to_string()));
}

#[test]
fn convert_standalone_issue_has_provider_source() {
    let item = CorrelationResult::Standalone(StandaloneResult::Issue {
        key: "42".to_string(),
        description: "Fix the bug".to_string(),
        source: "github".to_string(),
    });

    let proto = correlation_result_to_work_item(&item, &[]);
    assert_eq!(proto.source, Some("github".to_string()));
}

#[test]
fn convert_standalone_remote_branch_has_git_source() {
    let item = CorrelationResult::Standalone(StandaloneResult::RemoteBranch {
        branch: "origin/feat".to_string(),
    });

    let proto = correlation_result_to_work_item(&item, &[]);
    assert_eq!(proto.source, Some("git".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-core -- convert_correlated_checkout_has_hostname`
Expected: FAIL — `CorrelatedWorkItem` doesn't have a `source` field yet.

- [ ] **Step 3: Add `source` to `CorrelatedWorkItem` and update `StandaloneResult`**

In `crates/flotilla-core/src/data.rs`, modify `CorrelatedWorkItem` (line 56):

```rust
#[derive(Debug, Clone)]
pub struct CorrelatedWorkItem {
    pub anchor: CorrelatedAnchor,
    pub branch: Option<String>,
    pub description: String,
    pub linked_change_request: Option<String>,
    pub linked_session: Option<String>,
    pub linked_issues: Vec<String>,
    pub workspace_refs: Vec<String>,
    pub correlation_group_idx: usize,
    pub source: Option<String>,
}
```

Update `StandaloneResult::Issue` to carry `source` (line 68):

```rust
#[derive(Debug, Clone)]
pub enum StandaloneResult {
    Issue { key: String, description: String, source: String },
    RemoteBranch { branch: String },
}
```

Add a `source()` accessor to `CorrelationResult` (in the impl block near line 79):

```rust
pub fn source(&self) -> Option<&str> {
    match self {
        CorrelationResult::Correlated(c) => c.source.as_deref(),
        CorrelationResult::Standalone(StandaloneResult::Issue { source, .. }) => {
            Some(source.as_str())
        }
        CorrelationResult::Standalone(StandaloneResult::RemoteBranch { .. }) => Some("git"),
    }
}
```

- [ ] **Step 4: Populate `source` in `group_to_work_item`**

In `crates/flotilla-core/src/data.rs`, in `group_to_work_item` (around line 320), add source derivation before the final `Some(...)`:

```rust
let source = match &anchor {
    CorrelatedAnchor::Checkout(_) => {
        Some(gethostname::gethostname().to_string_lossy().into_owned())
    }
    CorrelatedAnchor::ChangeRequest(key) => providers
        .change_requests
        .get(key.as_str())
        .map(|cr| cr.provider_name.clone())
        .filter(|s| !s.is_empty()),
    CorrelatedAnchor::Session(key) => providers
        .sessions
        .get(key.as_str())
        .map(|s| s.provider_name.clone())
        .filter(|s| !s.is_empty()),
};

Some(CorrelationResult::Correlated(CorrelatedWorkItem {
    anchor,
    branch,
    description,
    linked_change_request,
    linked_session,
    linked_issues: Vec::new(),
    workspace_refs,
    correlation_group_idx: group_idx,
    source,
}))
```

- [ ] **Step 5: Populate `source` for standalone issues**

In the standalone issues loop (around line 465):

```rust
work_items.push(CorrelationResult::Standalone(StandaloneResult::Issue {
    key: id.clone(),
    description: issue.title.clone(),
    source: issue.provider_name.clone(),
}));
```

- [ ] **Step 6: Thread `source` through `correlation_result_to_work_item`**

In `crates/flotilla-core/src/convert.rs` (line 33), add `source` to the WorkItem:

```rust
WorkItem {
    kind,
    identity,
    branch: item.branch().map(|s| s.to_string()),
    description: item.description().to_string(),
    checkout,
    change_request_key: item.change_request_key().map(|s| s.to_string()),
    session_key: item.session_key().map(|s| s.to_string()),
    issue_keys: item.issue_keys().to_vec(),
    workspace_refs: item.workspace_refs().to_vec(),
    is_main_checkout: item.is_main_checkout(),
    debug_group,
    source: item.source().map(|s| s.to_string()),
}
```

- [ ] **Step 7: Fix all `CorrelatedWorkItem` and `StandaloneResult::Issue` construction sites**

Add `source: None` to all `CorrelatedWorkItem { ... }` in tests:
- `convert.rs` test `convert_correlated_checkout` (line 118)
- `data.rs` test helpers — especially the `correlated()` helper function (around line 750). This is critical because many tests use struct-update syntax `..correlated(...)`, so updating the helper propagates to all of them.

Add `source: String::new()` to all `StandaloneResult::Issue { ... }` in tests:
- `data.rs` test helper `issue_item` (around line 789)
- `convert.rs` test `convert_standalone_issue` (line 155)

- [ ] **Step 8: Run all tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat: populate WorkItem.source from provider data during conversion"
```

### Task 4: Rename cloud agent abbreviation from `Ses` to `Agt`

**Files:**
- Modify: `crates/flotilla-core/src/providers/coding_agent/mod.rs:21-25`

- [ ] **Step 1: Change the trait defaults**

In `crates/flotilla-core/src/providers/coding_agent/mod.rs`:

```rust
fn item_noun(&self) -> &str {
    "agent"
}
fn abbreviation(&self) -> &str {
    "Agt"
}
```

Note: Codex already overrides to `"task"` / `"Cdx"` — those are preserved. Cursor inherits the default, so it gets `"agent"` / `"Agt"`.

- [ ] **Step 2: Run tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-core/src/providers/coding_agent/mod.rs
git commit -m "refactor: rename cloud agent abbreviation from Ses to Agt"
```

## Chunk 3: Table UI — Source Column

### Task 5: Add Source column to table

**Files:**
- Modify: `crates/flotilla-tui/src/ui.rs:313-339` (header + widths)
- Modify: `crates/flotilla-tui/src/ui.rs:420-436` (`build_header_row`)
- Modify: `crates/flotilla-tui/src/ui.rs:438-543` (`build_item_row`)

- [ ] **Step 1: Add Source column header**

In `ui.rs` at line 313, insert `Cell::from("Source")` before `Cell::from("Path")`:

```rust
let header = Row::new(vec![
    Cell::from(""),
    Cell::from("Source"),   // NEW
    Cell::from("Path"),
    Cell::from("Description"),
    Cell::from("Branch"),
    Cell::from(labels.checkouts.abbr.as_str()),
    Cell::from("WS"),
    Cell::from(labels.code_review.abbr.as_str()),
    Cell::from(labels.sessions.abbr.as_str()),
    Cell::from("Issues"),
    Cell::from("Git"),
])
.style(Style::default().fg(Color::DarkGray).bold())
.height(1);
```

- [ ] **Step 2: Add Source column width**

Update the widths array (line 328):

```rust
let widths = [
    Constraint::Length(3),  // icon
    Constraint::Length(10), // Source
    Constraint::Fill(1),    // Path
    Constraint::Fill(2),    // Description
    Constraint::Fill(1),    // Branch
    Constraint::Length(3),  // WT
    Constraint::Length(3),  // WS
    Constraint::Length(4),  // PR
    Constraint::Length(4),  // SS
    Constraint::Length(6),  // Issues
    Constraint::Length(5),  // Git
];
```

- [ ] **Step 3: Add empty cell to `build_header_row`**

In `build_header_row` (line 420), add one more `Cell::from("")` to match 11 columns:

```rust
fn build_header_row(_header: &SectionHeader) -> Row<'static> {
    Row::new(vec![
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
    ])
    .height(1)
}
```

- [ ] **Step 4: Add source cell to `build_item_row` with elision**

In `build_item_row` (line 438), the function currently takes `(item, providers, col_widths, repo_root)`. Add a `prev_source: Option<&str>` parameter for elision:

Change signature to:
```rust
fn build_item_row<'a>(
    item: &WorkItem,
    providers: &ProviderData,
    col_widths: &[u16],
    repo_root: &Path,
    prev_source: Option<&str>,
) -> Row<'a> {
```

Extract source with elision:
```rust
let source_display = match item.source.as_deref() {
    Some(s) if prev_source == Some(s) => String::new(), // elide repeated
    Some(s) => s.to_string(),
    None => String::new(),
};
```

In the `Row::new(vec![...])` at line 510, insert the Source cell after the icon cell:

```rust
Row::new(vec![
    Cell::from(Span::styled(
        format!(" {icon}"),
        Style::default().fg(icon_color),
    )),
    Cell::from(Span::styled(
        source_display,
        Style::default().fg(Color::Indexed(245)),
    )),
    Cell::from(Span::styled(
        path_display,
        Style::default().fg(Color::Indexed(245)),
    )),
    // ... rest of cells unchanged
])
```

- [ ] **Step 5: Update column width index references**

The `col_widths` array is now 11 elements. Update index references in `build_item_row`:
- `path_width`: was `col_widths.get(1)`, now `col_widths.get(2)`
- `desc_width`: was `col_widths.get(2)`, now `col_widths.get(3)`
- `branch_width`: was `col_widths.get(3)`, now `col_widths.get(4)`

- [ ] **Step 6: Update the caller to track `prev_source` and pass it**

In `render_repo_table` (around line 348-370), the rows are built with a `.map()` closure. Change to track the previous source across items:

```rust
let mut prev_source: Option<String> = None;
let rows: Vec<Row> = rui
    .table_view
    .table_entries
    .iter()
    .map(|entry| {
        let is_multi_selected = if let GroupEntry::Item(ref item) = entry {
            rui.multi_selected.contains(&item.identity)
        } else {
            false
        };

        match entry {
            GroupEntry::Header(_header) => {
                prev_source = None; // reset on section boundary
                build_header_row(_header)
            }
            GroupEntry::Item(item) => {
                let mut row = build_item_row(
                    item,
                    &rm.providers,
                    &col_widths,
                    model.active_repo_root(),
                    prev_source.as_deref(),
                );
                prev_source = item.source.clone();
                if is_multi_selected {
                    row = row.style(Style::default().bg(Color::Indexed(236)));
                }
                row
            }
        }
    })
    .collect();
```

- [ ] **Step 7: Run tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/flotilla-tui/src/ui.rs
git commit -m "feat: add Source column to work item table with elision"
```

## Chunk 4: Preview Panel + Event Log

### Task 6: Update preview panel with provider-prefixed titles

**Files:**
- Modify: `crates/flotilla-tui/src/ui.rs:558-650` (`render_preview_content`)

- [ ] **Step 1: Extract a `capitalize` helper**

Add a small helper near the top of `ui.rs` or in `ui_helpers` to avoid repeating the capitalize pattern:

```rust
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}
```

- [ ] **Step 2: Update session preview**

In `render_preview_content` (line 606-618), change:

```rust
if let Some(ref ses_key) = item.session_key {
    if let Some(ses) = providers.sessions.get(ses_key.as_str()) {
        let provider_prefix = if ses.provider_name.is_empty() {
            "Agent".to_string()
        } else {
            format!("{} Agent", capitalize(&ses.provider_name))
        };
        lines.push(format!("{}: {}", provider_prefix, ses.title));
        lines.push(format!("Id: {}", ses_key));
        lines.push(format!("Status: {:?}", ses.status));
        if let Some(ref model_name) = ses.model {
            lines.push(format!("Model: {}", model_name));
        }
        if let Some(ref updated) = ses.updated_at {
            let display = updated.split('T').next().unwrap_or(updated);
            lines.push(format!("Updated: {}", display));
        }
    }
}
```

- [ ] **Step 3: Update PR preview**

In `render_preview_content` (line 594-604), change:

```rust
if let Some(ref pr_key) = item.change_request_key {
    if let Some(cr) = providers.change_requests.get(pr_key.as_str()) {
        let provider_prefix = if cr.provider_name.is_empty() {
            String::new()
        } else {
            format!("{} ", capitalize(&cr.provider_name))
        };
        lines.push(format!(
            "{}{} #{}: {}",
            provider_prefix,
            model.active_labels().code_review.abbr,
            pr_key,
            cr.title
        ));
        lines.push(format!("State: {:?}", cr.status));
    }
}
```

- [ ] **Step 4: Update issue preview**

In `render_preview_content` (line 631-639), change:

```rust
for issue_key in &item.issue_keys {
    if let Some(issue) = providers.issues.get(issue_key.as_str()) {
        let labels = issue.labels.join(", ");
        let provider_prefix = if issue.provider_name.is_empty() {
            String::new()
        } else {
            format!("{} ", capitalize(&issue.provider_name))
        };
        lines.push(format!(
            "{}Issue #{}: {} [{}]",
            provider_prefix, issue_key, issue.title, labels
        ));
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/flotilla-tui/src/ui.rs
git commit -m "feat: show provider name in preview panel titles"
```

### Task 7: Extend event log to capture provider field

**Files:**
- Modify: `crates/flotilla-tui/src/event_log.rs:230-244` (`MessageVisitor`)

- [ ] **Step 1: Write a test for provider prefix in log entries**

In `crates/flotilla-tui/src/event_log.rs` (or a new test module), add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_visitor_captures_provider() {
        // We can't easily test the tracing layer end-to-end in a unit test,
        // but we can verify the visitor struct behavior by constructing it
        // and checking it formats correctly.
        let display = format_log_message("hello", Some("claude"));
        assert_eq!(display, "[claude] hello");
    }

    #[test]
    fn message_visitor_no_provider() {
        let display = format_log_message("hello", None);
        assert_eq!(display, "hello");
    }
}
```

- [ ] **Step 2: Extract formatting into a testable function**

```rust
fn format_log_message(message: &str, provider: Option<&str>) -> String {
    if let Some(provider) = provider {
        format!("[{}] {}", provider, message)
    } else {
        message.to_string()
    }
}
```

- [ ] **Step 3: Extend `MessageVisitor` to capture provider**

Change the visitor from a tuple struct to named fields:

```rust
struct MessageVisitor {
    message: String,
    provider: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_string(),
            "provider" => self.provider = Some(value.to_string()),
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Update `on_event` to use the new struct and formatting function**

In the `on_event` method (around line 222-227):

```rust
let mut visitor = MessageVisitor {
    message: String::new(),
    provider: None,
};
event.record(&mut visitor);

let display = format_log_message(&visitor.message, visitor.provider.as_deref());
EVENT_LOG.lock().unwrap().push(level, display);
```

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/flotilla-tui/src/event_log.rs
git commit -m "feat: show [provider] prefix in event log entries"
```

### Task 8: Add `provider` field to provider log entries

**Files:**
- Modify: `crates/flotilla-core/src/providers/coding_agent/claude.rs`
- Modify: `crates/flotilla-core/src/providers/coding_agent/codex.rs`
- Modify: `crates/flotilla-core/src/providers/coding_agent/cursor.rs`
- Modify: `crates/flotilla-core/src/providers/code_review/github.rs`
- Modify: `crates/flotilla-core/src/providers/issue_tracker/github.rs`

- [ ] **Step 1: Audit and update log calls**

Search for `debug!`, `info!`, `warn!`, `error!` calls in each provider file. Add `provider = "name"` as a structured field before the message. Examples:

```rust
// Before:
debug!("Fetched {} sessions", sessions.len());
// After:
debug!(provider = "claude", "Fetched {} sessions", sessions.len());
```

Provider names per file:
- `claude.rs` → `provider = "claude"`
- `codex.rs` → `provider = "codex"`
- `cursor.rs` → `provider = "cursor"`
- `code_review/github.rs` → `provider = "github"`
- `issue_tracker/github.rs` → `provider = "github"`

- [ ] **Step 2: Run tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-core/src/providers/
git commit -m "feat: add provider field to structured log entries"
```

## Chunk 5: Final Verification

### Task 9: Full workspace verification

- [ ] **Step 1: Run formatter**

Run: `cargo fmt`

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run all tests**

Run: `cargo test --locked`
Expected: All pass

- [ ] **Step 4: Fix any issues found**

Address any clippy warnings or test failures.

- [ ] **Step 5: Final commit if needed**

```bash
git add -A
git commit -m "chore: fix clippy and formatting for provider attribution"
```
