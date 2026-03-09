# Persistent Terminal Sessions Design

**Issue:** [#32 — Persistent and portable terminal sessions](https://github.com/rjwittams/flotilla/issues/32)
**Related:** [#33 — Multi-host coordination](https://github.com/rjwittams/flotilla/issues/33)

## Problem

Workspaces are tied to a specific terminal multiplexer session. If that session ends, the workspace is gone. Terminal processes (shells, agents, build watchers) must be recreated from scratch, losing scrollback, state, and context.

## Core Insight

The current "workspace" concept conflates two things:

1. **Terminal processes** running on a host within a checkout — persistent, host-bound, the ground truth about what is happening.
2. **Views** that surface those processes into a visible layout — ephemeral, swappable, potentially remote.

Flotilla should own the process lifecycle. Workspace managers (cmux, zellij, tmux, future web views) become pure presentation layers.

## Design

### Data Model

**`ManagedTerminalId`** — structured identity, not a formatted string:

```rust
pub struct ManagedTerminalId {
    pub checkout: String,
    pub role: String,
    pub index: u32,
}
```

Display as `{checkout}/{role}/{index}` for logging/UI, match on fields.

**`ManagedTerminal`** — a persistent terminal process:

- `id: ManagedTerminalId`
- `role: String` — what kind of terminal (`shell`, `agent`, `build`, custom)
- `command: String` — what was launched
- `working_directory: PathBuf` — cwd, source of `CheckoutPath` correlation
- `status: TerminalStatus` — `Running`, `Exited(code)`, `Disconnected`
- `shpool_session: String` — underlying session name

### TerminalPool Provider Trait

```rust
#[async_trait]
pub trait TerminalPool: Send + Sync {
    async fn list_terminals(&self) -> Result<Vec<ManagedTerminal>, String>;
    async fn ensure_running(&self, id: &ManagedTerminalId, command: &str, cwd: &Path) -> Result<(), String>;
    async fn attach_command(&self, id: &ManagedTerminalId) -> Result<String, String>;
    async fn kill_terminal(&self, id: &ManagedTerminalId) -> Result<(), String>;
    async fn terminal_cwd(&self, id: &ManagedTerminalId) -> Result<PathBuf, String>;
}
```

**Two-step interaction with workspace managers:**

1. `ensure_running(id, command, cwd)` — make sure the process exists in the pool.
2. `attach_command(id)` — get the command the workspace manager should run in its pane (e.g., `shpool attach {session_name}`).

The workspace manager does not know about shpool. It asks "what command do I run?" and puts that in the pane.

### Implementations

**`ShpoolTerminalPool`** — the real implementation:

- Flotilla starts/manages a shpool server, socket under `~/.config/flotilla/shpool/`
- Session naming: `flotilla/{checkout}/{role}/{index}` maps from `ManagedTerminalId`
- `ensure_running()` starts a shpool session if not already alive
- `attach_command()` returns `shpool attach {session_name}`
- Sessions survive workspace manager disconnects and daemon restarts

**`PassthroughTerminalPool`** — degenerate fallback:

- `list_terminals()` returns empty
- `ensure_running()` is a no-op
- `attach_command()` returns the original command
- Best-effort; no persistence, no cwd tracking

### Template Evolution

The workspace template splits into two concerns:

**`content:`** — what to run (extensible beyond terminals):

```yaml
content:
  - role: shell
    type: terminal
    command: "$SHELL"
  - role: agent
    type: terminal
    command: "claude-code"
    count: 1
  - role: build
    type: terminal
    command: "cargo watch -x check"
```

**`layout:`** — how to display:

```yaml
layout:
  - slot: shell
  - slot: agent
    split: right
    overflow: tab
  - slot: build
    split: down
    parent: shell
    gap: placeholder
```

**Slot matching:** On workspace creation or reconnect, managed terminals are grouped by role and matched to layout slots.

- **Gap** (slot expects a role, none running): `placeholder` | `spawn` | `skip`
- **Overflow** (more terminals than slots): `tab` | `hide` | `expand`

### Crate Placement

| Component | Location |
|-----------|----------|
| `TerminalPool` trait, `ManagedTerminal`, `ManagedTerminalId` | `flotilla-core/src/providers/terminal/mod.rs` |
| `ShpoolTerminalPool` | `flotilla-core/src/providers/terminal/shpool.rs` |
| `PassthroughTerminalPool` | `flotilla-core/src/providers/terminal/passthrough.rs` |
| Protocol types | `flotilla-protocol/src/snapshot.rs` |
| Template format evolution | `flotilla-core/src/template.rs` |

### Changes to Existing Code

- **Provider registry** — add `TerminalPool` slot
- **Refresh cycle** — call `list_terminals()`, feed into correlation via `CheckoutPath`
- **Workspace managers** — use `ensure_running()` + `attach_command()` when a pool is available
- **Daemon startup** — start shpool server if shpool provider is selected
- **Template parsing** — support new `content:` / `layout:` split format

### Shpool Server Lifecycle

- Daemon starts shpool server on launch (or connects to existing one via socket)
- Shpool config/socket isolated under `~/.config/flotilla/shpool/`
- Daemon stops: leave shpool running for next launch
- Orphan cleanup: stale sessions via explicit `flotilla clean` or age-based policy

## Interaction with #33 (Multi-Host)

This design does not block multi-host coordination:

- The `TerminalPool` trait boundary is where remote pools would plug in later
- Multiple views can attach to the same pool (architecturally supported from day one)
- Agent migration is session log transfer, not process migration — a separate concern
- Read-only visibility of remote terminal pools fits naturally into the provider/refresh model

## Milestones

1. **Persistence end-to-end:** Kill workspace manager session, reopen flotilla, recreate workspace — shells reattach with scrollback intact.
2. **Migrate from cmux to zellij:** With terminal pool owning process lifecycle, the workspace manager is a thin view layer, making this swap straightforward.

## Investigation Prerequisite

Clone shpool to `~/dev`, evaluate its lib crate API to determine library vs CLI integration. Fork if necessary for minor improvements. Replace if it proves a poor fit.
