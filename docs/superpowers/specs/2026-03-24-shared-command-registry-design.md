# Shared Command Registry — Design Spec

**Issue:** #477
**Date:** 2026-03-24
**Related:** #401 (command palette phase 2), #397 (agent vs session naming), #474 (environment provisioning), #332 (command palette phase 1)

## Goal

A single command vocabulary shared across CLI, TUI command palette, TUI intents, step plans, web, MCP, and shell completions. Commands are the user-visible language of the system — every action flotilla takes is expressible as a command, and higher-level commands decompose into sequences of lower-level ones.

## Command Syntax

```
[target] noun [subject[,subject]*] verb [verb-specific args]
```

- **target** (optional): `host feta`, `env docker-3`. Omitted = inferred from context (env vars in a flotilla terminal, or local host).
- **noun**: what you're acting on — `workspace`, `checkout`, `cr`, `issue`, `agent`, `terminal`, `repo`, `branch`, `host`, `environment`.
- **subject** (optional): the specific instance(s). Can be a comma-separated set (`issue #1,#5,#7`). Type determined by noun. Omitted = inferred from TUI selection or context.
- **verb**: the action to perform on the noun.
- **verb-specific args**: additional arguments, format varies per verb.

### Examples

```
workspace feat-ws select
checkout my-feature create
checkout my-feature remove
cr #42 close
cr #42 link-issues #1,#5
issue #1,#5,#7 suggest-branch
agent claude-1 teleport
agent claude-1 archive
repo flotilla-org/cleat add
host feta checkout my-feature create        # explicit host target
terminal $uuid ensure                       # internal/step-level command
```

When subjects are inferred:
```
checkout create                             # subject from TUI selection
cr close                                    # subject from TUI selection
```

## Command Registry

The registry is a flat table of command definitions. Each entry specifies:

| Field | Purpose |
|-------|---------|
| noun | Category of thing being acted on |
| verb | Action name |
| description | Human-readable, for help/palette/logging |
| aliases | Alternative names (e.g., `pr` for `cr`) |
| subject_type | What the subject is — branch name, CR id, session id, etc. |
| subject_cardinality | Single, optional, set |
| additional_args | Verb-specific argument specs |
| availability | What context is needed (e.g., "requires a checkout", "requires a CR") |
| completion_sources | Per-argument completion source references |
| host_targetable | Whether this command can be prefixed with a host/environment target |
| level | `user` (CLI/palette visible) or `internal` (step-level, not typically invoked directly) |

### Levels

**User commands** appear in CLI help, palette, shell completions. These are the primary vocabulary.

**Internal commands** are the same data structure but not surfaced in help by default. They're the atomic building blocks that user commands decompose into. A user *can* invoke them (for debugging, manual stepping), but they're not the normal entry point. The `terminal ensure`, `environment create`, `attachable-set ensure` commands that are currently `StepAction` variants live here.

### Noun Vocabulary

| Noun | Subjects | Provider source |
|------|----------|-----------------|
| `workspace` | workspace ref/name | WorkspaceManager |
| `checkout` | branch name or path | Vcs, CheckoutManager |
| `cr` | CR id (aliases: `pr`) | ChangeRequestTracker |
| `issue` | issue id(s) | IssueTracker |
| `agent` | agent/session id | CloudAgentService (per #397) |
| `terminal` | attachable id | TerminalPool |
| `repo` | repo path or identity | config |
| `branch` | branch name | Vcs |
| `host` | host name | peer mesh |
| `environment` | environment id | EnvironmentProvider (per #474) |
| `attachable-set` | set id | AttachableStore |

### Current Intents → Commands

| TUI Intent | Command | Notes |
|------------|---------|-------|
| SwitchToWorkspace | `workspace <ref> select` | |
| CreateWorkspace | `workspace <checkout> create` | Local/remote distinction is execution, not command |
| RemoveCheckout | `checkout <branch> remove` | Currently fires FetchCheckoutStatus first (UI concern) |
| CreateCheckout | `checkout <branch> create` | `--fresh` flag for new branch vs existing |
| GenerateBranchName | `issue <ids> suggest-branch` | Set subject; AI utility call |
| OpenChangeRequest | `cr <id> open` | Opens in browser |
| CloseChangeRequest | `cr <id> close` | |
| OpenIssue | `issue <id> open` | Opens in browser |
| LinkIssuesToChangeRequest | `cr <id> link-issues <issue-ids>` | |
| TeleportSession | `agent <id> teleport` | Renamed from session per #397 |
| ArchiveSession | `agent <id> archive` | Renamed from session per #397 |

### Current StepActions → Commands

| StepAction | Command | Level |
|------------|---------|-------|
| CreateCheckout | `checkout <branch> create` | user |
| LinkIssuesToBranch | `branch <branch> link-issues <issue-ids>` | internal |
| RemoveCheckout | `checkout <branch> remove` | user |
| CreateWorkspaceForCheckout | `workspace <checkout> create` | user |
| CreateWorkspaceFromPreparedTerminal | `workspace <checkout> create-from-terminal` | internal |
| SelectWorkspace | `workspace <ref> select` | user |
| PrepareTerminalForCheckout | `terminal <checkout> prepare` | internal |
| FetchCheckoutStatus | `checkout <branch> status` | internal |
| ResolveAttachCommand | `agent <id> resolve-attach` | internal |
| EnsureCheckoutForTeleport | `checkout <branch> ensure-for-teleport` | internal |
| CreateTeleportWorkspace | `agent <id> create-teleport-workspace` | internal |
| ArchiveSession | `agent <id> archive` | user |
| GenerateBranchName | `issue <ids> suggest-branch` | user |
| OpenChangeRequest | `cr <id> open` | user |
| CloseChangeRequest | `cr <id> close` | user |
| OpenIssue | `issue <id> open` | user |
| LinkIssuesToChangeRequest | `cr <id> link-issues <issue-ids>` | internal |

### Current palette/global commands

| Palette entry | Command | Notes |
|---------------|---------|-------|
| refresh | `repo refresh` | Or `refresh` as a bare verb |
| search | `issue search <query>` | |
| add repo | `repo <path> add` | |
| quit, help, theme, layout, etc. | App-level actions | Not commands — pure UI state toggles, stay outside registry |

## Composition Model

A user command can decompose into a sequence of lower-level commands with named bindings. This replaces today's `build_plan()` match arms and the positional `prior` value threading.

### Plan Syntax

A plan is an ordered sequence of steps. Each step is a command invocation with bindings for inputs and outputs.

```
checkout $branch create
  → $checkout_path

branch $branch link-issues $issue_ids

terminal $checkout_path prepare
  → $attachable_set_id, $commands

workspace $checkout_path create-from-terminal $attachable_set_id $commands
```

`$name` references are resolved from a binding table. Outputs (after `→`) are stored in the binding table for subsequent steps.

### Plan Definitions

Plans are defined declaratively in the registry. A user command specifies its expansion:

```
"checkout create" expands to:
  1. checkout $branch create → $checkout_path
  2. branch $branch link-issues $issue_ids    [if $issue_ids present]
  3. workspace $checkout_path create
```

Conditional steps (step 2 above) execute only if their bindings are present. This replaces the current `StepOutcome::Skipped` pattern.

### Resource References

Commands produce resources identified by handle (ID). The binding carries the handle, not a snapshot. Consuming commands query the authoritative state from the store using the handle. The system may carry cached snapshots alongside handles as an optimization — this is transparent to commands.

### Target Routing

Each step has an implicit or explicit target (host/environment). The plan executor routes each step to its target. This replaces `StepHost::Local | Remote(HostName)` and eventually `StepHost::Environment(EnvironmentId)` from #474.

## Stepper / Plan Executor

The new stepper is a generic engine that replaces the current `StepAction`-specific executor.

**Execution loop:**
1. For each step in the plan:
   - Resolve input bindings from the binding table
   - Skip if conditional and bindings are absent
   - Determine target (host/environment) from command spec and context
   - Execute the command via its registered executor
   - Store outputs in the binding table
   - Log: the command text with resolved bindings IS the log entry

**What it gains:**
- No per-command match arms — dispatch through the registry
- Named bindings replace positional `prior` value threading
- Logging is automatic and human-readable
- Plans are inspectable and serializable (can show in TUI before/during execution)
- Manual stepping becomes possible (pause, inspect bindings, resume)

**Future extensions (not phase 1):**
- `par` — parallel execution of independent steps
- `seq` — explicit sequencing (default)
- `loop` — repeat over a set (e.g., for each terminal in a set)
- User-authored plans

## CLI Generation

The CLI is generated from the registry. Each noun becomes a subcommand group, verbs become subcommands within.

```
flotilla workspace <ws-ref> select
flotilla checkout <branch> create [--fresh]
flotilla cr <id> close
flotilla repo <path> add
flotilla refresh
```

**Phase 1 execution path:** The CLI parses using registry arg specs, produces a `CommandAction` via the command's legacy mapping, wraps in a `Command`, sends to the daemon. Identical to today's execution, entered through the registry instead of hand-written clap.

**Context inference:** Inside a flotilla terminal (`FLOTILLA_ATTACHABLE_ID` set), the CLI infers current repo, host, and environment. `flotilla cr close` without a subject means "close the CR for the current checkout in this terminal's context."

## Shell Completions

Generated from the registry via a hidden subcommand:

```
flotilla complete <line> <cursor-pos>
```

The completion engine walks the parse state:
1. No noun yet → complete with noun names
2. Noun entered → complete with subjects (dynamic, from completion source)
3. Noun + subject → complete with valid verbs
4. Verb entered → complete with verb-specific args

Shell-specific scripts (bash/zsh/fish) are boilerplate that calls `flotilla complete`.

### Completion Sources

```rust
trait CompletionSource: Send + Sync {
    async fn completions(&self, prefix: &str, context: &CompletionContext) -> Vec<CompletionItem>;
}
```

- **Static sources:** verb names, flag names — from registry metadata
- **Dynamic sources:** subjects — query daemon for workspace names, branch names, CR ids, etc.
- **Context-derived:** from TUI selection or env vars

Each noun's subject type maps to a provider-backed completion source.

## Palette Integration

The command palette (Phase 2, #401) consumes the registry:

- Typing a noun shows available verbs
- Typing a subject triggers completion from the noun's completion source
- The selected work item pre-fills subjects
- Selecting a command from the palette resolves and executes it

**Command echo:** When a TUI action is triggered by key binding (e.g., `d` for delete), the palette/status bar briefly shows the resolved command text: `checkout my-feature remove`. Using the TUI teaches the CLI vocabulary.

## Crate Structure

New crate: `flotilla-commands`

| Responsibility | In crate |
|---|---|
| Command definitions (noun, verb, args, description) | Yes |
| Argument types and subject specs | Yes |
| Completion source trait + static sources | Yes |
| Plan composition definitions | Yes |
| Registry (flat table of all commands) | Yes |
| CLI parser generation from registry | Yes |
| Shell completion generation | Yes |
| Legacy mapping to `CommandAction` | Yes (phase 1, removed later) |
| Plan executor / stepper | No — stays in `flotilla-core` |
| Dynamic completion providers | No — stays in `flotilla-core` (provider-backed) |
| Palette rendering | No — stays in `flotilla-tui` |

### Dependency Direction

```
flotilla-protocol (wire types: CommandAction, Command, etc.)
    ↑
flotilla-commands (registry, definitions, CLI, completions)
    ↑
flotilla-core (executor, providers, dynamic completions)
    ↑
flotilla-tui (palette rendering, key bindings)
```

## Phasing

### Phase 1: Registry + CLI

Define the command registry with all current user-visible commands. Generate CLI from registry, replacing hand-written clap. Shell completions from registry. Each command maps to existing `CommandAction` for execution — no stepper changes.

**Deliverables:**
- `flotilla-commands` crate with registry
- CLI generated from registry
- Shell completion via `flotilla complete`
- All current CLI functionality preserved

### Phase 2: Palette Integration

Command palette consumes registry for Phase 2 (#401). TUI intents become thin lookups into the registry. UI actions echo resolved commands.

**Deliverables:**
- Palette shows noun-verb commands with completions
- Key bindings resolve through registry
- Command echo in status bar

### Phase 3: Composition + New Stepper

Plans expressed as command sequences with named bindings. New generic stepper replaces `StepAction`-specific executor. `build_plan()` migrated to declarative composition.

**Deliverables:**
- Plan composition model in registry
- New stepper with binding resolution
- `build_plan()` → declarative plans
- Plan inspection/logging in TUI

### Phase 4: Step Dissolution

Step-level commands promoted to first-class registry entries at `internal` level. `StepAction` enum dissolved. All execution goes through the registry.

**Deliverables:**
- `StepAction` removed
- All commands (user and internal) in registry
- Full command-level logging and tracing

## Open Questions

- Exact verb-specific argument syntax (positional vs flags) — to be determined per command during implementation.
- Whether `host` and `environment` are pure target prefixes or also nouns with their own verbs (e.g., `host feta status`). Likely both.
- Interaction model for command echo — timing, duration, palette vs status bar.
