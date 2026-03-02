# cmux-controller v2 Design — Ratatui TUI Dashboard

A persistent TUI dashboard for managing development workspaces across cmux, worktrunk, GitHub, and Claude Code.

## Problem

Spinning up a development workspace involves juggling multiple tools: creating a worktree (`wt switch`), opening a cmux workspace with the right pane layout, launching Claude Code or other AI assistants, checking PR status, finding issues to work on. Each tool has its own CLI. There's no unified view of "what am I working on, and what should I work on next?"

## Solution

A standalone Rust binary (`cmux-controller`) that provides a persistent, tabbed TUI dashboard. It aggregates data from worktrunk, cmux, GitHub, and Claude Code into one interface, and dispatches actions back to those tools.

## Architecture

```
cmux-controller (Rust binary)
  TUI Layer (ratatui + crossterm)
    - Tab bar, list view, preview pane, action menu, status bar
  Data Layer (tokio async)
    - Fetches from: wt list, cmux list-workspaces, gh pr/issue list
    - Auto-refreshes on interval, manual refresh on 'r'
    - Parses JSON (serde) from CLI subprocess output
  Action Layer
    - Dispatches to: wt switch, cmux new-workspace/new-split/send, gh, claude
    - Applies workspace templates (.cmux/workspace.yaml)
```

All external tool interaction is via subprocess calls to `wt`, `cmux`, `gh`, and `claude` CLIs. No library-level integration.

## TUI Layout

```
+-- cmux-controller ---------------------------------------------------+
| [Worktrees] [PRs] [Issues] [Sessions]        myorg/myrepo    r:3s   |
+-------------------------------------------+--------------------------+
|  * fix-auth-bug       ^2  PR #342  +ws:3  | PR #342                  |
|  * update-api-docs    ^1  PR #340  +ws:5  | Fix auth redirect loop   |
|  o refactor-logger    ^3  --       --     | affecting login page     |
| > o old-feature       merged      --      | when session expires...  |
|                                           |                          |
|  + New worktree                           | +12 -3  2 files          |
+-------------------------------------------+--------------------------+
| enter:switch  d:remove  p:PR  space:menu  ?:help  q:quit            |
+----------------------------------------------------------------------+
```

- Tab bar: switch between data views
- Main list: items with status columns, vi-style navigation
- Preview pane: contextual detail for focused item
- Status bar: keybindings and refresh indicator

## Tabs

| Tab | Source | Items |
|-----|--------|-------|
| Worktrees | `wt list --format=json` joined with `cmux list-workspaces` | Active worktrees with branch, ahead/behind, PR link, cmux workspace indicator |
| PRs | `gh pr list --json number,title,headRefName,state,updatedAt` | Open PRs, marked if they have a local worktree |
| Issues | `gh issue list --json number,title,labels,updatedAt` | Open issues for the repo |
| Sessions | claude.ai API (stubbed) | Web sessions with branch info for teleport |

## Keybindings

| Key | Action | Context |
|-----|--------|---------|
| Tab / Shift-Tab | Switch tabs | Global |
| j/k or up/down | Navigate list | Global |
| Enter | Default action (switch/create) | All items |
| Space | Open action popup menu | All items |
| d | Remove worktree (`wt remove`) | Worktrees |
| p | Open PR in browser (`gh pr view --web`) | Worktrees with PR, PRs |
| n | New worktree (text input for branch name) | Worktrees |
| r | Force refresh | Global |
| ? | Toggle help overlay | Global |
| q / Esc | Quit (Esc also closes popups) | Global |

## Action Menu (Space)

Popup overlay with context-sensitive actions. Varies by item type:

Worktree with cmux workspace: Switch, Remove, View diff, Open PR, Close workspace
Worktree without workspace: Create workspace, Remove, View diff
PR without worktree: Checkout and create workspace, View in browser
Issue: Create branch and workspace, View in browser
Web session: Teleport into worktree

## Worktree-to-cmux Workspace Correlation

Match worktree paths against cmux workspace names from `cmux list-workspaces`. When cmux-controller creates a workspace, it names it after the branch for reliable matching later.

## Workspace Templates

Same format as v1 prototype, stored in `.cmux/workspace.yaml` in the repo:

```yaml
panes:
  - name: main
    surfaces:
      - command: "{main_command}"
  - name: ai
    split: right
    surfaces:
      - name: codex
        command: codex
      - name: gemini
        command: gemini
  - name: shell
    split: down
    parent: ai
    surfaces:
      - command: ""
```

Variables: `{main_command}`, `{branch}`, `{repo}`, `{issue_number}`, `{session_id}`

Applied via cmux CLI: `cmux new-workspace`, `cmux new-split`, `cmux new-surface`, `cmux send`.

## Workspace Creation Sequence

1. `wt switch [--create] <branch> --no-cd` to create/switch worktree
2. Get worktree path from `wt list --format=json`
3. Read and render template with variables
4. `cmux new-workspace` (name it after the branch)
5. For each pane: `cmux new-split <direction> [--panel <parent>]`
6. For each extra surface: `cmux new-surface --type terminal --pane <ref>`
7. For each surface: `cmux send --surface <ref> "cd <path> && <command>\n"`

## Data Refresh

- Auto-refresh every N seconds (default 5, configurable)
- Manual refresh with 'r'
- Async subprocess calls via tokio so TUI stays responsive
- Stale indicator if refresh takes longer than expected

## Control Center Workspace

cmux-controller itself runs in a dedicated cmux workspace:

```yaml
panes:
  - name: dashboard
    surfaces:
      - command: cmux-controller
  - name: claude
    split: right
    surfaces:
      - command: claude
```

## External Data Formats

### wt list --format=json

```json
[
  {
    "branch": "fix-auth-bug",
    "path": "/Users/me/dev/myrepo.fix-auth-bug",
    "main_state": "ahead",
    "main": { "ahead": 2, "behind": 0 },
    "remote": { "ahead": 0, "behind": 0 },
    "working_tree": { "staged": false, "modified": true },
    "is_current": false,
    "is_main": false,
    "statusline": "fix-auth-bug  ^2"
  }
]
```

### cmux list-workspaces

```
* workspace:14  scratch  [selected]
  workspace:10  next-job
  workspace:3   Main
```

Text format (no JSON). Parse with regex: `(\*?\s+)(workspace:\d+)\s+(.+?)(?:\s+\[selected\])?$`

### gh pr list --json

```json
[
  {
    "number": 342,
    "title": "Fix auth redirect",
    "headRefName": "fix-auth-bug",
    "state": "OPEN",
    "updatedAt": "2026-03-01T..."
  }
]
```

## Tech Stack

- ratatui + crossterm (TUI rendering and input)
- tokio (async subprocess execution, timers)
- serde + serde_json (JSON parsing)
- clap (CLI argument parsing)
- serde_yaml (workspace template parsing)
- dirs (XDG config directory resolution)
- toml (config file parsing)

## Config

`~/.config/cmux-controller/config.toml`:

```toml
refresh_interval = 5
default_repo = "~/dev/myrepo"

[cmux]
bin = "/Applications/cmux.app/Contents/Resources/bin/cmux"

[template]
default_path = ".cmux/workspace.yaml"
```

## Repo Structure

```
cmux-controller/
  Cargo.toml
  src/
    main.rs           -- entry point, arg parsing, app loop
    app.rs            -- App state, event handling
    ui/
      mod.rs          -- layout, rendering
      tabs.rs         -- tab bar
      list.rs         -- main item list
      preview.rs      -- preview pane
      popup.rs        -- action menu overlay
      help.rs         -- help overlay
    data/
      mod.rs          -- DataStore, refresh logic
      worktrees.rs    -- wt list parsing
      workspaces.rs   -- cmux list-workspaces parsing
      prs.rs          -- gh pr list parsing
      issues.rs       -- gh issue list parsing
      sessions.rs     -- web session fetching (stubbed)
    actions/
      mod.rs          -- action dispatch
      worktree.rs     -- wt switch, wt remove
      workspace.rs    -- cmux workspace creation from template
      template.rs     -- template loading and rendering
    config.rs         -- config file parsing
  .cmux/
    workspace.yaml    -- example template
```

## What Carries Over from Python Prototype

- Workspace template format and variable substitution
- Data source logic (wt, gh, cmux CLI calls)
- Action dispatch sequence (wt switch -> cmux new-workspace -> cmux send)
- Web session API stub

## Deferred

- Web session API (depends on claude.ai/code)
- Multi-repo support (tabs per repo)
- cmux read-screen for Claude interaction
- Publishing to crates.io
