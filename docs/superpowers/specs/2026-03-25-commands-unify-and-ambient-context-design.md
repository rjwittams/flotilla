# Commands Unify and Ambient Context — Design Spec

**Issues:** #502 (unify queries and commands), #505 (RequiresRepoContext), #506 (ambient context on command definitions)
**Date:** 2026-03-25
**Parent spec:** `docs/superpowers/specs/2026-03-24-shared-command-registry-design.md`
**Phase 1 spec:** `docs/superpowers/specs/2026-03-24-shared-command-registry-phase1-design.md`
**Phase 2 spec:** `docs/superpowers/specs/2026-03-25-shared-command-registry-phase2-design.md` (in flotilla.shared-command-registry)

## Goal

Simplify `Resolved` so the Phase 2 palette has a single dispatch path with correct host and repo context resolution. Queries become `CommandAction` variants routed through the same `Command` path as mutations. Each command declares what ambient context it needs — repo and host resolution — via typed metadata on `Resolved`.

## Implementation Order

1. **#502** — Unify queries and commands. Changes the `Resolved` shape, adds query `CommandAction` variants, simplifies dispatch.
2. **#506** — Ambient context. Reshapes `Resolved` to carry context requirements via `HostResolution`. Subsumes #505.

## #502: Unify Queries and Commands

### Problem

`Resolved` has 10 variants: one for `Command` and nine for queries (`RepoDetail`, `RepoProviders`, `RepoWork`, `HostList`, `HostStatus`, `HostProviders`, plus three host-routed repo variants). Each query variant requires its own dispatch arm in `main.rs`, its own standalone function in `cli.rs`, its own `DaemonHandle` method, its own `Request`/`Response` pair, and its own `RequestDispatcher` arm. Host-routed repo queries (`HostRepoDetail`, `HostRepoProviders`, `HostRepoWork`) return "not yet supported."

The `set_host()` method on `Resolved` has a 9-arm match that promotes query variants to host-routed variants.

### Design

Queries become `CommandAction` variants. They flow through `execute()`, are handled as daemon-level actions in `InProcessDaemon`, and return results as `CommandValue` variants. Host-routed queries work via the existing peer command forwarding — no new infrastructure.

### Protocol changes (`flotilla-protocol/src/commands.rs`)

New `CommandAction` variants:

```rust
QueryRepoDetail { repo: RepoSelector }
QueryRepoProviders { repo: RepoSelector }
QueryRepoWork { repo: RepoSelector }
QueryHostList
QueryHostStatus { host: String }
QueryHostProviders { host: String }
```

New `CommandValue` variants:

```rust
RepoDetail(RepoDetailResponse)
RepoProviders(RepoProvidersResponse)
RepoWork(RepoWorkResponse)
HostList(HostListResponse)
HostStatus(HostStatusResponse)
HostProviders(HostProvidersResponse)
```

Remove `Request` variants: `GetRepoDetail`, `GetRepoProviders`, `GetRepoWork`, `ListHosts`, `GetHostStatus`, `GetHostProviders`. Remove their `Response` counterparts.

### DaemonHandle trait (`flotilla-core/src/daemon.rs`)

Remove noun-verb query methods:

- `get_repo_detail()`, `get_repo_providers()`, `get_repo_work()`
- `list_hosts()`, `get_host_status()`, `get_host_providers()`

Keep infrastructure methods: `get_state`, `list_repos`, `get_status`, `get_topology`, `subscribe`, `replay_since`, `execute`, `cancel`.

The removed methods become private helpers inside `InProcessDaemon`, called by `execute()` when it handles Query\* actions. `SocketDaemon` loses these methods too — the CLI sends queries as Commands through `execute()`.

### InProcessDaemon::execute() (`flotilla-core/src/in_process.rs`)

Query\* actions are handled alongside existing daemon-level commands (TrackRepoPath, Refresh, etc.):

```rust
CommandAction::QueryRepoDetail { repo } => {
    let result = self.repo_detail_internal(&repo).await;
    // emit CommandStarted, CommandFinished { result: CommandValue::RepoDetail(result) }
}
// same pattern for other Query* variants
```

These never reach the per-repo `build_plan()` in `executor.rs`.

### Resolved (`flotilla-commands/src/resolved.rs`)

Collapses from 10 variants to 2:

```rust
pub enum Resolved {
    Command(Command),
    RequiresRepoContext(Command),
}
```

`RequiresRepoContext` wraps commands that need `--repo` / `FLOTILLA_REPO` injection (checkout create, issue search). The SENTINEL pattern (`RepoSelector::Query("")`) stays in the `CommandAction` fields — `inject_repo_context` matches on it. The `Resolved` variant makes the requirement type-level.

### set_host() simplification

```rust
pub fn set_host(&mut self, host: String) {
    match self {
        Resolved::Command(cmd) | Resolved::RequiresRepoContext(cmd) => {
            cmd.host = Some(HostName::new(&host));
        }
    }
}
```

### Noun resolve() changes

**RepoNoun** (`commands/repo.rs`):
- `Providers` → `Resolved::Command(Command { action: QueryRepoProviders { repo: RepoSelector::Query(slug) }, .. })`
- `Work` → `Resolved::Command(Command { action: QueryRepoWork { repo: RepoSelector::Query(slug) }, .. })`
- Subject only (no verb) → `Resolved::Command(Command { action: QueryRepoDetail { repo: RepoSelector::Query(slug) }, .. })`

**HostNoun** (`commands/host.rs`):
- `List` → `Resolved::Command(Command { action: QueryHostList, .. })`
- `Status` → `Resolved::Command(Command { action: QueryHostStatus { host }, .. })`
- `Providers` → `Resolved::Command(Command { action: QueryHostProviders { host }, .. })`
- `Route(inner)` → resolve inner, call `set_host()` — which now just sets `Command.host`

**CheckoutNoun** (`commands/checkout.rs`):
- `Create` → `Resolved::RequiresRepoContext(cmd)` (SENTINEL for repo)

**IssueNoun** (`commands/issue.rs`):
- `Search` → `Resolved::RequiresRepoContext(cmd)` (SENTINEL for repo)

### CLI dispatch (`main.rs`)

```rust
match resolved {
    Resolved::Command(cmd) => run_control_command(cli, cmd, format).await,
    Resolved::RequiresRepoContext(mut cmd) => {
        inject_repo_context(&mut cmd, cli)?;
        run_control_command(cli, cmd, format).await
    }
}
```

All query and command dispatch goes through `run_control_command` → `run_command`.

### CLI output (`flotilla-tui/src/cli.rs`)

Delete standalone functions: `run_repo_detail`, `run_repo_providers`, `run_repo_work`, `run_host_list`, `run_host_status`, `run_host_providers`.

`run_command` gains result formatting for the new `CommandValue` variants. The existing human/JSON formatters move into the `CommandFinished` handler.

### RequestDispatcher (`flotilla-daemon/src/server/request_dispatch.rs`)

Remove arms for the deleted `Request` variants. Query traffic now arrives as `Request::Execute { command }` and routes through `RemoteCommandRouter` like any other command.

### Host-routed queries

`host feta repo myslug providers` resolves to `Command { host: Some("feta"), action: QueryRepoProviders { repo } }`. `RemoteCommandRouter` forwards it to feta via the peer mesh. The remote daemon handles it in `execute()` and returns `CommandValue::RepoProviders(...)` via the peer event stream. The "not yet supported" error goes away.

### TUI impact

None. The TUI reads from `AppModel` via snapshots/deltas. It never sends query commands or interacts with `Resolved`.

## #506: Ambient Context on Command Definitions

### Problem

Commands that need ambient context (repo identity, target host) rely on implicit conventions: SENTINEL empty strings in `CommandAction` fields, six TUI command builders (`repo_command`, `targeted_command`, `item_host_command`, etc.) that each encode a different host resolution strategy. Nothing declares what context a command needs. The Phase 2 palette requires a generic dispatch path that fills context uniformly.

### Design

Each command declares its context requirements via `Resolved::NeedsContext`. A `HostResolution` enum encodes why a host is needed, and the CLI or TUI resolves it from its environment. No `--target-host` flag or environment variable — CLI users use `host <name> ...` syntax for remote targeting.

### HostResolution enum (`flotilla-commands/src/resolved.rs`)

```rust
pub enum HostResolution {
    /// No host needed — runs locally.
    Local,
    /// The user's chosen provisioning target (TUI: ui.target_host; CLI: host routing).
    ProvisioningTarget,
    /// The host where the subject item lives.
    SubjectHost,
    /// The host where the provider runs (remote-only repos route to provider host).
    ProviderHost,
}
```

### Resolved reshapes

```rust
pub enum Resolved {
    Ready(Command),
    NeedsContext {
        command: Command,
        repo: bool,
        host: HostResolution,
    },
}
```

`RequiresRepoContext(cmd)` from #502 becomes `NeedsContext { command: cmd, repo: true, host: HostResolution::Local }`.

### Context table

| Command | repo | host |
|---|---|---|
| Checkout (create) | true (SENTINEL) | ProvisioningTarget |
| RemoveCheckout / FetchCheckoutStatus | false | SubjectHost |
| OpenChangeRequest / CloseChangeRequest | false | ProviderHost |
| OpenIssue | false | ProviderHost |
| ArchiveSession | false | ProviderHost |
| GenerateBranchName | true | ProvisioningTarget |
| SearchIssues | true (SENTINEL) | Local |
| SelectWorkspace | false | Local |
| TeleportSession | false | Local |
| TrackRepoPath / UntrackRepo / Refresh | false | Local |
| Query\* variants | depends on query | Local (host set by routing) |

Commands where all context is already resolved (e.g., `repo myslug checkout main` where the repo is explicit, or query commands with explicit RepoSelector) return `Ready(cmd)`.

### Noun resolve() changes

Each noun's `resolve()` returns the appropriate `Resolved` variant per the context table. Examples:

```rust
// checkout create — needs repo and provisioning target
Resolved::NeedsContext {
    command: Command { action: CommandAction::Checkout { repo: RepoSelector::Query("".into()), .. }, .. },
    repo: true,
    host: HostResolution::ProvisioningTarget,
}

// cr open — needs provider host (no repo)
Resolved::NeedsContext {
    command: Command { action: CommandAction::OpenChangeRequest { id }, .. },
    repo: false,
    host: HostResolution::ProviderHost,
}

// repo myslug providers — fully resolved query
Resolved::Ready(Command { action: CommandAction::QueryRepoProviders { repo: RepoSelector::Query(slug) }, .. })
```

### set_host() update

```rust
pub fn set_host(&mut self, host: String) {
    match self {
        Resolved::Ready(cmd) => cmd.host = Some(HostName::new(&host)),
        Resolved::NeedsContext { command, .. } => command.host = Some(HostName::new(&host)),
    }
}
```

### CLI dispatch (`main.rs`)

```rust
match resolved {
    Resolved::Ready(cmd) => run_control_command(cli, cmd, format).await,
    Resolved::NeedsContext { mut command, repo, .. } => {
        if repo {
            inject_repo_context(&mut command, cli)?;
        }
        // ProvisioningTarget: host already set if wrapped in `host <name> ...`
        // SubjectHost / ProviderHost: N/A in CLI (no item context) — runs locally
        run_control_command(cli, command, format).await
    }
}
```

### TUI impact

None in this issue. Phase 2 introduces `resolve_host(HostResolution, Option<&WorkItem>, &App)` and `tui_dispatch(Resolved, ...)` when the palette and intent adapter need them. The existing command builders stay unchanged.

### #464 alignment

`HostResolution` is resolved at the CLI/TUI edge to a concrete host name stored in `Command.host`. Today the daemon forwards the entire command to that host. When #464 (step-level remote routing) lands, `build_plan()` reads `Command.host` and stamps steps with `StepHost::Remote(host)` instead. The field population stays the same; the daemon's interpretation changes. `HostResolution` categories map naturally to step routing patterns:

| HostResolution | Step routing |
|---|---|
| ProvisioningTarget | Checkout + terminal steps remote, workspace step local |
| SubjectHost | Operation steps target the item's host |
| ProviderHost | Provider interaction steps target the provider's host |
| Local | All steps local |

## Testing

### #502 tests

- **Resolve round-trips:** Update existing noun resolve tests. Expected `Resolved` variants change from `RepoDetail { slug }` to `Command(Command { action: QueryRepoDetail { repo } })` etc.
- **Display round-trips:** Noun Display/parse tests pass unchanged (noun structs don't change).
- **Query execution:** New tests verify Query\* CommandActions through `InProcessDaemon::execute()` produce correct CommandValue results.
- **Host-routed queries:** Test `host feta repo myslug providers` end-to-end — resolves, routes via peer forwarding, returns result.
- **CLI output:** Test that `run_command` formats query CommandValue variants correctly for human and JSON output.
- **Snapshot tests:** Investigate before accepting — Resolved shape changes may affect serialized test data.

### #506 tests

- **Context table coverage:** Each noun's `resolve()` produces the correct `HostResolution` and `repo` flag per the table.
- **CLI dispatch:** `NeedsContext` with `repo: true` fails without `--repo` / `FLOTILLA_REPO`, succeeds with it.
- **Ready vs NeedsContext:** Commands with explicit context (e.g., `repo myslug checkout main`) return `Ready`, not `NeedsContext`.

### Not tested here

- TUI `resolve_host` / `tui_dispatch` — Phase 2
- Intent adapter round-trips — Phase 2

## Crate Boundaries

| Change | Crate |
|---|---|
| Query CommandAction/CommandValue variants | `flotilla-protocol` |
| Remove Request/Response query variants | `flotilla-protocol` |
| Remove DaemonHandle query methods | `flotilla-core` |
| Query handling in InProcessDaemon::execute() | `flotilla-core` |
| SocketDaemon: remove query method impls | `flotilla-client` |
| Resolved reshaping, HostResolution | `flotilla-commands` |
| Noun resolve() updates | `flotilla-commands` |
| CLI dispatch simplification | `flotilla` (main.rs) |
| CLI output: delete standalone runners | `flotilla-tui` (cli.rs) |
| RequestDispatcher: remove query arms | `flotilla-daemon` |

No changes to `flotilla-tui` app code, widgets, or intent system.

## Scope

### Delivers

- Queries are CommandAction variants routed through execute()
- Resolved collapses to Ready / NeedsContext
- Host-routed queries work end-to-end
- DaemonHandle trait has one command path (execute)
- Each command declares repo and host context requirements
- CLI dispatch fills context uniformly

### Defers

- `--target-host` CLI flag and `FLOTILLA_TARGET_HOST` env var (use `host <name> ...` syntax)
- TUI resolve_host / tui_dispatch (Phase 2)
- Intent adapter migration (Phase 2)
- Step-level remote routing (#464)
