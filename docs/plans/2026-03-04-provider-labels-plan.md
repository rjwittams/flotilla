# Provider Labels Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace hard-coded provider-specific terminology ("Worktrees", "Pull Requests", "PR") with provider-supplied labels throughout the UI, intents, and logs.

**Architecture:** Four provider traits gain `section_label()`, `item_noun()`, `abbreviation()` with generic defaults. Labels are resolved per-repo during refresh and stored as `RepoLabels` on `AppModel`. All display sites read from this struct instead of hard-coding terms.

**Tech Stack:** Rust, ratatui TUI, async_trait provider traits

---

### Task 1: Add label methods to provider traits

**Files:**
- Modify: `src/providers/vcs/mod.rs:19-26` (CheckoutManager trait)
- Modify: `src/providers/code_review/mod.rs:7-16` (CodeReview trait)
- Modify: `src/providers/issue_tracker/mod.rs:7-13` (IssueTracker trait)
- Modify: `src/providers/coding_agent/mod.rs:6-14` (CodingAgent trait)

**Step 1: Add default-impl methods to CheckoutManager**

In `src/providers/vcs/mod.rs`, add to the `CheckoutManager` trait:

```rust
#[async_trait]
pub trait CheckoutManager: Send + Sync {
    #[allow(dead_code)]
    fn display_name(&self) -> &str;
    fn section_label(&self) -> &str { "Checkouts" }
    fn item_noun(&self) -> &str { "checkout" }
    fn abbreviation(&self) -> &str { "CO" }
    async fn list_checkouts(&self, repo_root: &Path) -> Result<Vec<Checkout>, String>;
    async fn create_checkout(&self, repo_root: &Path, branch: &str) -> Result<Checkout, String>;
    async fn remove_checkout(&self, repo_root: &Path, branch: &str) -> Result<(), String>;
}
```

**Step 2: Add default-impl methods to CodeReview**

In `src/providers/code_review/mod.rs`:

```rust
#[async_trait]
pub trait CodeReview: Send + Sync {
    #[allow(dead_code)]
    fn display_name(&self) -> &str;
    fn section_label(&self) -> &str { "Change Requests" }
    fn item_noun(&self) -> &str { "change request" }
    fn abbreviation(&self) -> &str { "CR" }
    async fn list_change_requests(&self, repo_root: &Path, limit: usize) -> Result<Vec<ChangeRequest>, String>;
    #[allow(dead_code)]
    async fn get_change_request(&self, repo_root: &Path, id: &str) -> Result<ChangeRequest, String>;
    async fn open_in_browser(&self, repo_root: &Path, id: &str) -> Result<(), String>;
    async fn list_merged_branch_names(&self, repo_root: &Path, limit: usize) -> Result<Vec<String>, String>;
}
```

**Step 3: Add default-impl methods to IssueTracker**

In `src/providers/issue_tracker/mod.rs`:

```rust
#[async_trait]
pub trait IssueTracker: Send + Sync {
    #[allow(dead_code)]
    fn display_name(&self) -> &str;
    fn section_label(&self) -> &str { "Issues" }
    fn item_noun(&self) -> &str { "issue" }
    fn abbreviation(&self) -> &str { "#" }
    async fn list_issues(&self, repo_root: &Path, limit: usize) -> Result<Vec<Issue>, String>;
    async fn open_in_browser(&self, repo_root: &Path, id: &str) -> Result<(), String>;
}
```

**Step 4: Add default-impl methods to CodingAgent**

In `src/providers/coding_agent/mod.rs`:

```rust
#[async_trait]
pub trait CodingAgent: Send + Sync {
    #[allow(dead_code)]
    fn display_name(&self) -> &str;
    fn section_label(&self) -> &str { "Sessions" }
    fn item_noun(&self) -> &str { "session" }
    fn abbreviation(&self) -> &str { "Ses" }
    async fn list_sessions(&self, criteria: &RepoCriteria) -> Result<Vec<CloudAgentSession>, String>;
    async fn archive_session(&self, session_id: &str) -> Result<(), String>;
    #[allow(dead_code)]
    async fn attach_command(&self, session_id: &str) -> Result<String, String>;
}
```

**Step 5: Run check**

Run: `cargo check 2>&1`
Expected: compiles with existing warnings only

**Step 6: Commit**

```bash
git add src/providers/vcs/mod.rs src/providers/code_review/mod.rs src/providers/issue_tracker/mod.rs src/providers/coding_agent/mod.rs
git commit -m "feat: add section_label/item_noun/abbreviation to provider traits"
```

---

### Task 2: Add provider-specific overrides

**Files:**
- Modify: `src/providers/vcs/wt.rs:107-110` (WtCheckoutManager impl)
- Modify: `src/providers/code_review/github.rs:103-106` (GitHubCodeReview impl)

**Step 1: Override labels on WtCheckoutManager**

In `src/providers/vcs/wt.rs`, inside the `impl super::CheckoutManager for WtCheckoutManager` block, after `display_name()`:

```rust
    fn display_name(&self) -> &str {
        "wt"
    }

    fn section_label(&self) -> &str { "Worktrees" }
    fn item_noun(&self) -> &str { "worktree" }
    fn abbreviation(&self) -> &str { "WT" }
```

**Step 2: Override labels on GitHubCodeReview**

In `src/providers/code_review/github.rs`, inside the `impl super::CodeReview for GitHubCodeReview` block, after `display_name()`:

```rust
    fn display_name(&self) -> &str {
        "GitHub Pull Requests"
    }

    fn section_label(&self) -> &str { "Pull Requests" }
    fn item_noun(&self) -> &str { "pull request" }
    fn abbreviation(&self) -> &str { "PR" }
```

**Step 3: Run check**

Run: `cargo check 2>&1`
Expected: compiles cleanly

**Step 4: Commit**

```bash
git add src/providers/vcs/wt.rs src/providers/code_review/github.rs
git commit -m "feat: override labels for wt (Worktrees) and GitHub (Pull Requests)"
```

---

### Task 3: Add CategoryLabels and RepoLabels to AppModel

**Files:**
- Modify: `src/app/model.rs`

**Step 1: Add the label types and field**

Add at the top of `src/app/model.rs`, after the existing imports:

```rust
#[derive(Clone, Debug)]
pub struct CategoryLabels {
    pub section: String,
    pub noun: String,
    pub abbr: String,
}

impl CategoryLabels {
    /// Capitalize the noun for use in titles: "worktree" -> "Worktree"
    pub fn noun_capitalized(&self) -> String {
        let mut c = self.noun.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().to_string() + c.as_str(),
        }
    }
}

impl Default for CategoryLabels {
    fn default() -> Self {
        Self {
            section: "—".into(),
            noun: "item".into(),
            abbr: "".into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RepoLabels {
    pub checkouts: CategoryLabels,
    pub code_review: CategoryLabels,
    pub issues: CategoryLabels,
    pub sessions: CategoryLabels,
}
```

Add to `AppModel`:

```rust
pub struct AppModel {
    pub repos: HashMap<PathBuf, RepoModel>,
    pub repo_order: Vec<PathBuf>,
    pub active_repo: usize,
    pub provider_statuses: HashMap<(PathBuf, String, String), ProviderStatus>,
    pub status_message: Option<String>,
    pub labels: HashMap<PathBuf, RepoLabels>,
}
```

Add a convenience accessor to `AppModel`:

```rust
    pub fn active_labels(&self) -> &RepoLabels {
        static DEFAULT: std::sync::LazyLock<RepoLabels> = std::sync::LazyLock::new(RepoLabels::default);
        self.labels.get(&self.repo_order[self.active_repo]).unwrap_or(&DEFAULT)
    }
```

**Step 2: Run check**

Run: `cargo check 2>&1`
Expected: compiles (labels field not yet populated, just added)

**Step 3: Commit**

```bash
git add src/app/model.rs
git commit -m "feat: add CategoryLabels, RepoLabels, and labels field to AppModel"
```

---

### Task 4: Populate labels in refresh_all

**Files:**
- Modify: `src/app/executor.rs:260-308` (refresh_all function)

**Step 1: Add label population after registry is restored**

In `executor.rs::refresh_all`, after the line `rm.registry = registry;` and after the issues-disabled block, add:

```rust
        // Populate labels from provider traits
        let repo_labels = super::model::RepoLabels {
            checkouts: rm.registry.checkout_managers.values().next()
                .map(|cm| super::model::CategoryLabels {
                    section: cm.section_label().into(),
                    noun: cm.item_noun().into(),
                    abbr: cm.abbreviation().into(),
                })
                .unwrap_or_default(),
            code_review: rm.registry.code_review.values().next()
                .map(|cr| super::model::CategoryLabels {
                    section: cr.section_label().into(),
                    noun: cr.item_noun().into(),
                    abbr: cr.abbreviation().into(),
                })
                .unwrap_or_default(),
            issues: rm.registry.issue_trackers.values().next()
                .map(|it| super::model::CategoryLabels {
                    section: it.section_label().into(),
                    noun: it.item_noun().into(),
                    abbr: it.abbreviation().into(),
                })
                .unwrap_or_default(),
            sessions: rm.registry.coding_agents.values().next()
                .map(|ca| super::model::CategoryLabels {
                    section: ca.section_label().into(),
                    noun: ca.item_noun().into(),
                    abbr: ca.abbreviation().into(),
                })
                .unwrap_or_default(),
        };
        app.model.labels.insert(path.clone(), repo_labels);
```

**Step 2: Run check**

Run: `cargo check 2>&1`
Expected: compiles (labels populated but not consumed yet)

**Step 3: Commit**

```bash
git add src/app/executor.rs
git commit -m "feat: populate RepoLabels from provider traits during refresh"
```

---

### Task 5: Convert SectionHeader to string wrapper and pass labels to correlate

**Files:**
- Modify: `src/data.rs:33-51` (SectionHeader enum + Display)
- Modify: `src/data.rs:267` (correlate signature)
- Modify: `src/data.rs:94` (refresh calls correlate)
- Modify: `src/ui.rs:338-352` (build_header_row)

**Step 1: Replace SectionHeader enum with newtype**

In `src/data.rs`, replace the `SectionHeader` enum and its `Display` impl (lines 33-51) with:

```rust
#[derive(Debug, Clone)]
pub struct SectionHeader(pub String);

impl fmt::Display for SectionHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

Also remove the `PartialEq` derive from the old enum — the new struct doesn't need it.

**Step 2: Add SectionLabels struct and update correlate signature**

Add above `correlate()`:

```rust
pub struct SectionLabels {
    pub checkouts: String,
    pub code_review: String,
    pub issues: String,
    pub sessions: String,
}

impl Default for SectionLabels {
    fn default() -> Self {
        Self {
            checkouts: "Checkouts".into(),
            code_review: "Change Requests".into(),
            issues: "Issues".into(),
            sessions: "Sessions".into(),
        }
    }
}
```

Change `correlate` signature from `fn correlate(&mut self)` to `fn correlate(&mut self, labels: &SectionLabels)`.

**Step 3: Update all SectionHeader constructions in correlate**

Replace the five `SectionHeader::Variant` constructions in `correlate()`:

- `SectionHeader::Checkouts` → `SectionHeader(labels.checkouts.clone())`
- `SectionHeader::Sessions` → `SectionHeader(labels.sessions.clone())`
- `SectionHeader::PullRequests` → `SectionHeader(labels.code_review.clone())`
- `SectionHeader::RemoteBranches` → `SectionHeader("Remote Branches".into())`
- `SectionHeader::Issues` → `SectionHeader(labels.issues.clone())`

**Step 4: Update refresh() to build and pass labels**

In `refresh()` (line ~184), replace `self.correlate();` with:

```rust
        let section_labels = SectionLabels {
            checkouts: registry.checkout_managers.values().next()
                .map(|cm| cm.section_label().to_string())
                .unwrap_or_else(|| "Checkouts".into()),
            code_review: registry.code_review.values().next()
                .map(|cr| cr.section_label().to_string())
                .unwrap_or_else(|| "Change Requests".into()),
            issues: registry.issue_trackers.values().next()
                .map(|it| it.section_label().to_string())
                .unwrap_or_else(|| "Issues".into()),
            sessions: registry.coding_agents.values().next()
                .map(|ca| ca.section_label().to_string())
                .unwrap_or_else(|| "Sessions".into()),
        };
        self.correlate(&section_labels);
```

**Step 5: Run check**

Run: `cargo check 2>&1`
Expected: compiles. The `build_header_row` in ui.rs already uses `format!("── {} ──", header)` which calls `Display`, so it works unchanged.

**Step 6: Commit**

```bash
git add src/data.rs
git commit -m "refactor: replace SectionHeader enum with string wrapper, pass labels to correlate"
```

---

### Task 6: Update column headers in ui.rs

**Files:**
- Modify: `src/ui.rs:262-274` (table header row)

**Step 1: Replace hard-coded column headers**

Change the header row construction at line 262 from hard-coded strings to label-driven:

```rust
    let labels = model.active_labels();
    let header = Row::new(vec![
        Cell::from(""),
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

Note: "WS" (workspace), "Issues", and "Git" remain hard-coded — they aren't provider-specific abbreviations.

**Step 2: Run check**

Run: `cargo check 2>&1`
Expected: compiles

**Step 3: Commit**

```bash
git add src/ui.rs
git commit -m "feat: use provider labels for table column headers"
```

---

### Task 7: Update intent labels

**Files:**
- Modify: `src/app/intent.rs:18-31` (Intent::label method)
- Modify: `src/app/intent.rs:47-53` (Intent::shortcut_hint method)
- Modify: any callers of `label()` and `shortcut_hint()`

**Step 1: Change label() to take RepoLabels**

Change `Intent::label` from returning `&'static str` to taking labels and returning `String`:

```rust
    pub fn label(&self, labels: &super::model::RepoLabels) -> String {
        match self {
            Intent::SwitchToWorkspace => "Switch to workspace".into(),
            Intent::CreateWorkspace => "Create workspace".into(),
            Intent::RemoveWorktree => format!("Remove {}", labels.checkouts.noun),
            Intent::CreateWorktreeAndWorkspace => format!("Create {} + workspace", labels.checkouts.noun),
            Intent::GenerateBranchName => "Generate branch name".into(),
            Intent::OpenPr => format!("Open {} in browser", labels.code_review.noun),
            Intent::OpenIssue => "Open issue in browser".into(),
            Intent::TeleportSession => "Teleport session".into(),
            Intent::ArchiveSession => "Archive session".into(),
        }
    }
```

**Step 2: Change shortcut_hint() similarly**

```rust
    pub fn shortcut_hint(&self, labels: &super::model::RepoLabels) -> Option<String> {
        match self {
            Intent::RemoveWorktree => Some(format!("d:remove {}", labels.checkouts.noun)),
            Intent::OpenPr => Some(format!("p:show {}", labels.code_review.abbr)),
            _ => None,
        }
    }
```

**Step 3: Update all callers**

Search for `.label()` and `.shortcut_hint()` calls on Intent. Update each to pass the labels:

In `src/ui.rs` (action menu rendering and shortcut hints) — these will need `model.active_labels()` passed through.

In `src/app/mod.rs` (if any calls exist) — same treatment.

Run: `cargo check 2>&1` — fix any compile errors from changed signatures.

**Step 4: Commit**

```bash
git add src/app/intent.rs src/ui.rs src/app/mod.rs
git commit -m "feat: intent labels use provider-supplied terminology"
```

---

### Task 8: Update help text, dialog titles, and preview/status messages in ui.rs

**Files:**
- Modify: `src/ui.rs:755` (delete dialog title)
- Modify: `src/ui.rs:779-781` (help text)
- Modify: `src/ui.rs:534` (preview: "PR #...")
- Modify: `src/ui.rs:582` (debug panel: "PR()")
- Modify: `src/ui.rs:599` (debug panel: CorItemKind label)
- Modify: `src/ui.rs:704,712` (delete dialog: "PR:", "No PR found")

**Step 1: Update delete dialog title**

Line 755: change `" Remove Worktree "` to use labels:

```rust
    let title = format!(" Remove {} ", model.active_labels().checkouts.noun_capitalized());
    let paragraph = Paragraph::new(lines)
        .block(Block::bordered().title(title))
        .wrap(Wrap { trim: true });
```

**Step 2: Update help text**

Lines 779-781: change the hard-coded worktree/PR references:

```rust
        Line::from(format!("  n                New branch (enter name, creates {})", labels.checkouts.noun)),
        Line::from(format!("  d                Remove {} (with safety check)", labels.checkouts.noun)),
        Line::from(format!("  p                Show {} in browser", labels.code_review.abbr)),
```

This requires `let labels = model.active_labels();` at the top of `render_help`. Change `render_help` signature to take `model: &AppModel` in addition to `ui: &UiState`.

**Step 3: Update preview panel**

Line 534: change `format!("PR #{}: {}", cr.id, cr.title)` to:

```rust
format!("{} #{}: {}", model.active_labels().code_review.abbr, cr.id, cr.title)
```

**Step 4: Update delete confirmation panel**

Line 704: change `Span::raw("  PR: ")` to use labels:
```rust
Span::raw(format!("  {}: ", model.active_labels().code_review.abbr))
```

Line 712: change `"  No PR found"` to:
```rust
format!("  No {} found", model.active_labels().code_review.abbr)
```

Note: the delete confirmation rendering function may need `model` passed if it doesn't already have it.

**Step 5: Update debug panel labels**

Line 582: `format!("PR({}/{})", provider, id)` → `format!("CR({}/{})", provider, id)` (debug panel uses internal terminology, not provider labels — CR is the generic internal term for ChangeRequest).

Line 599: `CorItemKind::ChangeRequest => "PR"` → `CorItemKind::ChangeRequest => "CR"`.

**Step 6: Run check and fix any signature issues**

Run: `cargo check 2>&1`
Fix any functions that need `model` passed through for label access.

**Step 7: Commit**

```bash
git add src/ui.rs
git commit -m "feat: use provider labels in help text, dialogs, preview, and debug panel"
```

---

### Task 9: Update executor log messages

**Files:**
- Modify: `src/app/executor.rs` (log messages)

**Step 1: Update log messages to use labels**

Access labels via `app.model.active_labels()` for the active repo, or look up by path for non-active repos.

Line 71: `info!("deleting worktree {}", info.branch)` →
```rust
info!("deleting {} {}", app.model.active_labels().checkouts.noun, info.branch)
```

Line 85: `debug!("opening PR {id} in browser")` →
```rust
debug!("opening {} {id} in browser", app.model.active_labels().code_review.abbr)
```

Line 99: `info!("creating worktree {branch}")` →
```rust
info!("creating {} {branch}", app.model.active_labels().checkouts.noun)
```

Line 108: `info!("created worktree at {}", checkout.path.display())` →
```rust
info!("created {} at {}", app.model.active_labels().checkouts.noun, checkout.path.display())
```

**Step 2: Run check**

Run: `cargo check 2>&1`
Expected: compiles

**Step 3: Commit**

```bash
git add src/app/executor.rs
git commit -m "feat: executor logs use provider-supplied terminology"
```

---

### Task 10: Final verification

**Step 1: Run full check suite**

Run: `cargo check && cargo clippy && cargo test`
Expected: all pass, no new warnings beyond existing dead_code ones

**Step 2: Manual smoke test**

Run: `cargo run` and verify:
- Table section headers show "Worktrees" (not "Checkouts") when wt provider is active
- Column header shows "PR" (not "CR") when GitHub code review is active
- Help text shows "Remove worktree" and "Show PR in browser"
- Delete dialog title shows "Remove Worktree"
- Provider panel is unchanged

**Step 3: Commit any fixups**

```bash
git add -A
git commit -m "chore: provider labels cleanup and verification"
```
