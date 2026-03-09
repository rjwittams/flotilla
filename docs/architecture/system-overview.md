# System Overview

Flotilla is a multi-repo development fleet manager. It collects state from
version control, checkout managers, code review systems, issue trackers, coding
agents, and terminal workspaces, then presents those fragments as one row per
unit of work.

## Crate Layout

- `crates/flotilla-protocol`: shared serde-friendly wire and snapshot types.
- `crates/flotilla-core`: provider traits and implementations, refresh,
  correlation, issue cache, templates, command execution, config, and the
  in-process daemon.
- `crates/flotilla-daemon`: Unix-socket server wrapper around the core daemon.
- `crates/flotilla-tui`: protocol client, TUI app state, and rendering.
- `src/main.rs`: process bootstrap and mode selection.

## Runtime Model

Flotilla tracks a set of repositories. Each repo has:

- a detected `ProviderRegistry`
- a background `RepoRefreshHandle`
- daemon-owned repo state
- a published snapshot stream

The main flow is:

1. Detect providers for each repo from the environment, repo config, and
   available tools.
2. Refresh provider data concurrently.
3. Correlate the provider fragments into logical work items.
4. Convert the correlated state into protocol snapshots or deltas.
5. Let clients rebuild presentation state from the daemon stream.

## Durable Boundaries

### Providers gather raw facts

Provider implementations talk to external tools and normalize their output into
shared provider types. They should not own TUI state or pre-merge data into UI
rows.

### Core derives shared state

`flotilla-core` owns correlation, issue-cache injection, snapshot assembly, and
command execution. This is where raw provider data becomes the system model that
all clients share.

### Protocol flattens the model

`flotilla-protocol` is the serialization boundary. It defines `Command`,
`CommandResult`, `DaemonEvent`, `Snapshot`, `SnapshotDelta`, and the shared
provider data types.

### Clients own presentation

The TUI keeps local UI concerns such as active repo, selection, input mode,
unseen-change badges, and in-flight command display. It does not own provider
discovery, refresh orchestration, or the canonical issue cache.

## Repo Lifecycle

### Provider detection

Provider detection is per-repo, not global. Different repos may resolve to
different remote hosts, checkout strategies, or available providers.

### Refresh

Each repo refreshes on a 10-second interval and on demand. Refresh gathers
provider data in parallel, computes correlation groups, and publishes a new
`RefreshSnapshot`.

### Snapshot publication

The daemon polls refresh handles, injects cached issues, rebuilds correlated
work items, computes either a full snapshot or a delta, and broadcasts the
result to clients.

## Commands

Commands cross the daemon boundary as protocol values.

Current model:

- The client submits a `Command`.
- The daemon returns a command ID immediately for long-running work.
- `CommandStarted` and `CommandFinished` events describe lifecycle.
- The daemon refreshes repo state after execution.

Issue viewport and issue search commands are handled inline today because they
mutate daemon-owned cache state rather than producing a long-running
user-visible command result.

## Why The System Is Shaped This Way

- Multi-client support needs a daemon-owned source of truth.
- External tool integrations are volatile, so provider traits isolate them.
- Correlation is expensive and domain-specific, so it belongs in core rather
  than each frontend.
- The protocol boundary leaves room for more than one client, including future
  frontends.
