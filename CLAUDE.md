# Flotilla

TUI dashboard for managing development workspaces across git worktrees, code review (GitHub PRs), issue trackers, cloud agent sessions (Claude), and terminal multiplexers (cmux).

## Quick Reference

```bash
cargo build                          # build
cargo test                           # all tests
cargo clippy                         # lint
cargo run -- --repo-root /path       # run with explicit repo
cargo run                            # run, auto-detect repo from cwd
```

Always typecheck (`cargo check`) and run tests before making a PR.

## Architecture

Provider-based plugin system with data correlation:

```
Providers (git, wt, github, claude, cmux)
  → DataStore (raw: checkouts, PRs, issues, sessions)
    → Correlation engine (union-find groups items by shared keys)
      → WorkItems (correlated work units)
        → UI (ratatui table + tabs)
```

User actions flow: **Intent → Command → Executor → Provider call → Refresh**

### Key Modules

| Path | Role |
|------|------|
| `src/main.rs` | Entry point, event loop, mouse handling |
| `src/app/mod.rs` | `App` struct, key/mouse dispatch, mode transitions |
| `src/app/model.rs` | `AppModel` (repos, labels), `RepoModel` (per-repo data) |
| `src/app/command.rs` | `Command` enum, `CommandQueue` |
| `src/app/intent.rs` | `Intent` enum (available actions per work item) |
| `src/app/executor.rs` | Executes commands against providers |
| `src/app/ui_state.rs` | `UiState`, `TabId`, `UiMode`, per-repo UI state |
| `src/data.rs` | `DataStore`, `WorkItem`, `TableEntry`, correlation + table building |
| `src/ui.rs` | All ratatui rendering |
| `src/providers/` | Provider traits, implementations, registry, discovery, correlation |
| `src/config.rs` | Persistence to `~/.config/flotilla/` |
| `src/template.rs` | `.flotilla/workspace.yaml` pane templates |
| `src/event.rs` | Terminal event stream (key, mouse, tick) |
| `src/event_log.rs` | In-app tracing log with level filtering |

### Provider Traits

Each trait lives in `src/providers/<category>/mod.rs` with implementations alongside:

- **Vcs** + **CheckoutManager** (`vcs/git.rs`, `vcs/wt.rs`)
- **CodeReview** (`code_review/github.rs`)
- **IssueTracker** (`issue_tracker/github.rs`)
- **CodingAgent** (`coding_agent/claude.rs`)
- **AiUtility** (`ai_utility/claude.rs`)
- **WorkspaceManager** (`workspace/cmux.rs`)

Every provider trait has label methods: `section_label()`, `item_noun()`, `abbreviation()`, `display_name()`. Override defaults in implementations for custom terminology.

### Correlation

Union-find over `CorrelationKey` values (`Branch`, `CheckoutPath`, `ChangeRequestRef`, `SessionRef`). Items sharing any key merge into a single `WorkItem`. Issues link post-correlation via `AssociationKey` (don't cause merges). Tests in `src/providers/correlation.rs`.

## Conventions

- **Commits**: `type: lowercase description` — types: feat, fix, refactor, chore, docs. Present tense, no period.
- **Errors**: Provider methods return `Result<T, String>`. App-level uses `color_eyre::Result`.
- **Async**: `async-trait` for provider traits, `tokio::join!` for parallel refresh.
- **Enums over bools**: Prefer enum variants for state (e.g. `UiMode`, `Intent`, `WorkItemKind`).
- **Imports**: std first, external crates, then `use crate::...`.
- **Adding dependencies is fine** when they solve a real problem — don't reinvent the wheel.
- **Correctness first**: Always favour correct solutions over "pragmatic" shortcuts. Get the architecture right rather than patching around structural problems.

## UI Modes

`Normal` → `Help` → `Config` → `ActionMenu` → `BranchInput` → `FilePicker` → `DeleteConfirm`

Key bindings: `j/k` navigate, `Enter` execute, `Space` menu, `d` delete, `p` open PR, `n` new branch, `r` refresh, `[/]` switch tabs, `{/}` reorder tabs, `q` quit, `?` help.

## Config

Stored in `~/.config/flotilla/`:
- `repos/*.toml` — one per tracked repo (`path = "..."`)
- `tab-order.json` — array of repo paths

Workspace templates: `.flotilla/workspace.yaml` in repo root.
