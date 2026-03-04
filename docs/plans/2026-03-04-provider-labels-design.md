# Provider Labels Design

## Problem

Generic code uses provider-specific language throughout the UI. Section headers say "Worktrees" and "Pull Requests" regardless of which provider is active. Intent labels, help text, dialog titles, column headers, and log messages all hard-code these terms. If a GitLab code review provider were added, the UI would still say "Pull Requests" and "PR".

There is no mechanism for providers to supply their own UI labels. `display_name()` exists but only appears in the provider panel.

## Design

### Trait Label Methods

Four provider traits gain three methods each, all with generic default implementations:

| Trait | `section_label()` | `item_noun()` | `abbreviation()` |
|---|---|---|---|
| `CheckoutManager` | "Checkouts" | "checkout" | "CO" |
| `CodeReview` | "Change Requests" | "change request" | "CR" |
| `IssueTracker` | "Issues" | "issue" | "#" |
| `CodingAgent` | "Sessions" | "session" | "Ses" |

Concrete overrides:

| Implementation | `section_label()` | `item_noun()` | `abbreviation()` |
|---|---|---|---|
| `WtCheckoutManager` | "Worktrees" | "worktree" | "WT" |
| `GitHubCodeReview` | "Pull Requests" | "pull request" | "PR" |

All other current implementations use the defaults (no override needed).

Traits without table sections (`Vcs`, `AiUtility`, `WorkspaceManager`) keep only `display_name()`.

### RepoLabels on AppModel

Resolved labels are stored per-repo on `AppModel`, not on `DataStore`:

```rust
#[derive(Clone, Debug)]
pub struct CategoryLabels {
    pub section: String,  // "Worktrees", "Pull Requests"
    pub noun: String,     // "worktree", "pull request"
    pub abbr: String,     // "WT", "PR"
}

#[derive(Clone, Debug, Default)]
pub struct RepoLabels {
    pub checkouts: CategoryLabels,
    pub code_review: CategoryLabels,
    pub issues: CategoryLabels,
    pub sessions: CategoryLabels,
}
```

Populated in `refresh_all()` from the registry after each repo refreshes. Default when no provider is registered for a category.

### Label Flow

Labels flow from provider traits through `AppModel` to all display sites:

- **Section headers**: `SectionHeader` changes from an enum with hard-coded Display to `SectionHeader(pub String)`. `correlate()` receives section label strings as parameters.
- **Column headers**: `ui.rs` reads `labels.code_review.abbr` instead of hard-coded `"PR"`.
- **Intent labels**: `Intent::label()` takes `&RepoLabels` and produces "Remove worktree", "Open pull request in browser", etc.
- **Help text**: reads `item_noun()` for shortcut descriptions.
- **Dialog titles**: "Remove Worktree" uses capitalized `noun`.
- **Log messages**: executor and provider logs use the provider-specific terms from labels.
- **Provider panel**: unchanged, still uses `display_name()`.

### What Changes Where

| File | Change |
|---|---|
| `providers/vcs/mod.rs` | Add `section_label()`, `item_noun()`, `abbreviation()` to `CheckoutManager` with defaults |
| `providers/code_review/mod.rs` | Same for `CodeReview` |
| `providers/issue_tracker/mod.rs` | Same for `IssueTracker` |
| `providers/coding_agent/mod.rs` | Same for `CodingAgent` |
| `providers/vcs/wt.rs` | Override: "Worktrees", "worktree", "WT" |
| `providers/code_review/github.rs` | Override: "Pull Requests", "pull request", "PR" |
| `app/model.rs` | Add `CategoryLabels`, `RepoLabels`, `labels: HashMap<PathBuf, RepoLabels>` |
| `app/executor.rs` | Populate `model.labels` in `refresh_all()`. Use labels in log messages. |
| `data.rs` | `SectionHeader(String)` instead of enum. `correlate()` takes label strings. Remove `SectionHeader` Display enum. |
| `app/intent.rs` | `Intent::label()` takes `&RepoLabels`, uses `noun` fields |
| `ui.rs` | Column headers, dialog titles, help text, status messages read from labels |
| `main.rs` | No changes |
