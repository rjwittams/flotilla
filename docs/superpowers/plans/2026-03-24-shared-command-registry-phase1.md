# Shared Command Registry Phase 1 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a `flotilla-commands` crate with a shared command registry that defines flotilla's noun-verb command vocabulary. Generate the CLI from the registry, replacing hand-written clap. Add shell completions. All existing CLI functionality preserved.

**Architecture:** The registry is a flat table of `CommandDef` structs, each describing a noun+verb command with argument specs. A `CliBuilder` generates clap `Command` from the registry. Each `CommandDef` has a `resolve` function that maps parsed args to a `flotilla_protocol::Command` for daemon execution. Infrastructure subcommands (daemon, watch, hook/hooks) stay outside the registry as they're not domain commands. Shell completions use a hidden `complete` subcommand that walks the registry.

**Tech Stack:** Rust, clap (programmatic API, not derive), flotilla-protocol types

**Spec:** `docs/superpowers/specs/2026-03-24-shared-command-registry-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` (workspace root) | Modify | Add `flotilla-commands` to workspace members + root dependencies |
| `crates/flotilla-commands/Cargo.toml` | Create | Crate manifest |
| `crates/flotilla-commands/src/lib.rs` | Create | Crate root — re-exports |
| `crates/flotilla-commands/src/registry.rs` | Create | `CommandDef`, `NounDef`, `Registry` — the core data structures |
| `crates/flotilla-commands/src/commands/mod.rs` | Create | Module root for command definitions |
| `crates/flotilla-commands/src/commands/repo.rs` | Create | repo add, remove, refresh |
| `crates/flotilla-commands/src/commands/checkout.rs` | Create | checkout create, remove |
| `crates/flotilla-commands/src/commands/workspace.rs` | Create | workspace select, create |
| `crates/flotilla-commands/src/commands/cr.rs` | Create | cr open, close, link-issues |
| `crates/flotilla-commands/src/commands/issue.rs` | Create | issue open, suggest-branch, search |
| `crates/flotilla-commands/src/commands/agent.rs` | Create | agent teleport, archive |
| `crates/flotilla-commands/src/commands/host.rs` | Create | host as target prefix + list, status, providers |
| `crates/flotilla-commands/src/cli.rs` | Create | `CliBuilder` — generates clap `Command` tree from registry |
| `crates/flotilla-commands/src/complete.rs` | Create | Shell completion engine |
| `src/main.rs` | Modify | Replace clap derive with registry-generated CLI |

---

### Task 1: Create the flotilla-commands crate with core registry types

**Files:**
- Create: `crates/flotilla-commands/Cargo.toml`
- Create: `crates/flotilla-commands/src/lib.rs`
- Create: `crates/flotilla-commands/src/registry.rs`
- Modify: `Cargo.toml` (workspace root)

Define the core types that all command definitions use:

- [ ] **Step 1: Create the crate skeleton**

`crates/flotilla-commands/Cargo.toml`:
```toml
[package]
name = "flotilla-commands"
version = "0.1.0"
edition = "2021"
license.workspace = true

[dependencies]
flotilla-protocol = { path = "../flotilla-protocol" }
clap = { version = "4", features = ["string"] }
```

Add to workspace `Cargo.toml` members and root package dependencies.

- [ ] **Step 2: Define the core registry types**

In `registry.rs`, define:

```rust
/// A noun in the command vocabulary (workspace, checkout, cr, etc.)
pub struct NounDef {
    pub name: &'static str,
    pub aliases: &'static [&'static str],       // e.g. "pr" for "cr"
    pub description: &'static str,
}

/// A single command: noun + verb + argument spec + resolver.
pub struct CommandDef {
    pub noun: &'static str,
    pub verb: &'static str,
    pub description: &'static str,
    pub level: CommandLevel,
    pub subject: SubjectSpec,
    pub args: &'static [ArgSpec],
    pub resolve: ResolveFn,                      // parsed args → Command
}

pub enum CommandLevel { User, Internal }

/// What kind of subject this command takes.
pub enum SubjectSpec {
    None,                                         // creation command
    Single { name: &'static str, required: bool },
    Set { name: &'static str },                   // comma-separated set
}

pub struct ArgSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: ArgKind,
}

pub enum ArgKind {
    Flag,                                         // --fresh
    Named { value_name: &'static str },           // --branch <name>
    Positional { value_name: &'static str },      // trailing args
}

/// Resolution context provided to the resolve function.
pub struct ResolveContext {
    pub subject: Option<String>,
    pub subjects: Vec<String>,                    // for set subjects
    pub args: std::collections::HashMap<String, String>,
    pub host: Option<String>,                     // target prefix
    pub json: bool,
}

/// The resolve function signature.
pub type ResolveFn = fn(&ResolveContext) -> Result<Resolved, String>;

/// What a resolved command produces.
pub enum Resolved {
    /// Send this command to the daemon.
    DaemonCommand(flotilla_protocol::Command),
    /// A query that needs special handling (status, topology, etc.)
    Query(QueryKind),
}

pub enum QueryKind {
    RepoQuery { slug: String, detail: Option<String> },
    HostList,
    HostQuery { host: String, detail: String },
}

/// The registry — a flat list of all commands.
pub struct Registry {
    pub nouns: Vec<NounDef>,
    pub commands: Vec<CommandDef>,
}

impl Registry {
    pub fn commands_for_noun(&self, noun: &str) -> impl Iterator<Item = &CommandDef>;
    pub fn find(&self, noun: &str, verb: &str) -> Option<&CommandDef>;
    pub fn user_commands(&self) -> impl Iterator<Item = &CommandDef>;
}
```

- [ ] **Step 3: Write tests for Registry lookup**

Test `commands_for_noun`, `find`, `user_commands` filtering. Use a small test registry with 2-3 stub commands.

- [ ] **Step 4: Run tests, commit**

Run: `cargo test -p flotilla-commands`
Commit: `feat: create flotilla-commands crate with core registry types`

---

### Task 2: Define the domain command modules

**Files:**
- Create: `crates/flotilla-commands/src/commands/mod.rs`
- Create: `crates/flotilla-commands/src/commands/repo.rs`
- Create: `crates/flotilla-commands/src/commands/checkout.rs`
- Create: `crates/flotilla-commands/src/commands/workspace.rs`
- Create: `crates/flotilla-commands/src/commands/cr.rs`
- Create: `crates/flotilla-commands/src/commands/issue.rs`
- Create: `crates/flotilla-commands/src/commands/agent.rs`
- Create: `crates/flotilla-commands/src/commands/host.rs`

Each module exports a function that returns the `NounDef` and a `Vec<CommandDef>` for that noun. The `resolve` function on each `CommandDef` maps parsed args to `flotilla_protocol::Command` or a `QueryKind`.

- [ ] **Step 1: Implement repo commands**

`repo add`, `repo remove`, `repo refresh`, `repo <slug> (query)`, `repo <slug> providers`, `repo <slug> work`, `repo <slug> checkout <branch>`, `repo <slug> checkout --fresh <branch>`, `repo <slug> prepare-terminal <path>`.

Mirror the current `parse_repo_command` logic. The resolve function produces `CommandAction::TrackRepoPath`, `UntrackRepo`, `Refresh`, `Checkout`, `PrepareTerminalForCheckout`, or `QueryKind::RepoQuery`.

Test each resolve function independently: given a `ResolveContext`, assert the correct `Command`/`QueryKind` is produced.

- [ ] **Step 2: Implement checkout commands**

`checkout <path-or-branch> remove`.

Mirror the current `Checkout { checkout, command: CheckoutSubCommand::Remove }` mapping. Resolve produces `CommandAction::RemoveCheckout`.

- [ ] **Step 3: Implement host commands**

`host list`, `host <name> status`, `host <name> providers`, `host <name> refresh [repo]`, `host <name> repo ...`, `host <name> checkout <branch> remove`.

Host is both a target prefix and a noun. Mirror `parse_host_command` / `parse_host_control_command`. The host name sets `Command.host`.

- [ ] **Step 4: Implement workspace, cr, issue, agent commands**

These are new CLI commands that don't exist yet in the current CLI (they're TUI-only intents today). Define them with resolve functions that produce the right `CommandAction`:

- `workspace <ref> select` → `CommandAction::SelectWorkspace`
- `cr <id> open` → `CommandAction::OpenChangeRequest`
- `cr <id> close` → `CommandAction::CloseChangeRequest`
- `cr <id> link-issues <issue-ids>` → `CommandAction::LinkIssuesToChangeRequest`
- `issue <id> open` → `CommandAction::OpenIssue`
- `issue <ids> suggest-branch` → `CommandAction::GenerateBranchName`
- `agent <id> teleport` → `CommandAction::TeleportSession`
- `agent <id> archive` → `CommandAction::ArchiveSession`

These require `context_repo` to be set. For Phase 1, the resolve function can leave it as `None` (the daemon will need a way to infer it, or we accept that these CLI commands need a `--repo` flag initially).

- [ ] **Step 5: Build the full registry**

In `commands/mod.rs`, create `pub fn build_registry() -> Registry` that assembles all noun/command definitions.

- [ ] **Step 6: Tests for each resolve function**

Each command module should have tests verifying that `resolve(&context)` produces the expected `Command` variant. This is the key correctness guarantee — the registry produces the same `Command` values as the current hand-written CLI parsing.

- [ ] **Step 7: Run tests, commit**

Run: `cargo test -p flotilla-commands`
Commit: `feat: define domain command modules with resolve functions`

---

### Task 3: Build the CLI generator

**Files:**
- Create: `crates/flotilla-commands/src/cli.rs`

Generate a clap `Command` tree from the registry. Each noun becomes a subcommand group, each verb becomes a subcommand within it.

- [ ] **Step 1: Implement CliBuilder**

```rust
pub struct CliBuilder<'a> {
    registry: &'a Registry,
}

impl<'a> CliBuilder<'a> {
    pub fn new(registry: &'a Registry) -> Self;

    /// Build a clap Command tree for all user-level registry commands.
    /// Returns a Vec of top-level noun subcommands to add to the main CLI.
    pub fn build_noun_subcommands(&self) -> Vec<clap::Command>;

    /// Parse matched args from a clap ArgMatches into a ResolveContext,
    /// then call the CommandDef's resolve function.
    pub fn resolve_matches(&self, noun: &str, matches: &clap::ArgMatches) -> Result<Resolved, String>;
}
```

The builder iterates the registry's nouns and commands, creating clap `Command` and `Arg` entries from `SubjectSpec` and `ArgSpec`.

Key mapping:
- `SubjectSpec::Single { required: true }` → positional arg
- `SubjectSpec::Single { required: false }` → optional positional
- `SubjectSpec::Set` → positional with `num_args(1..)`
- `ArgKind::Flag` → `Arg::new().long().action(SetTrue)`
- `ArgKind::Named` → `Arg::new().long().value_name()`
- `ArgKind::Positional` → `Arg::new().value_name()`

Handle the noun-subject-verb ordering: the subject comes before the verb. In clap terms, the subject is a positional arg on the noun subcommand, and verbs are sub-subcommands. But this creates a parsing ambiguity (is the next token a subject or a verb?). Two approaches:

**Option A:** Verbs as sub-subcommands with subject as a preceding positional. Clap can handle this if all verbs are known strings — the subject is any token that isn't a verb.

**Option B:** Custom parsing similar to current `parse_repo_command` — match verb tokens manually from the arg list. This is what the existing CLI already does for `repo`.

Recommend **Option B** for Phase 1 — use clap for top-level noun routing but do verb+subject parsing manually within each noun handler. This avoids fighting clap's subcommand model and matches the existing pattern. The registry's `CommandDef` entries drive the manual parsing.

- [ ] **Step 2: Test CLI generation**

Test that `build_noun_subcommands()` produces the expected clap structure. Test that `resolve_matches()` correctly extracts subjects and args and calls the right resolve function.

- [ ] **Step 3: Run tests, commit**

Commit: `feat: CLI generator from command registry`

---

### Task 4: Wire registry CLI into the binary

**Files:**
- Modify: `src/main.rs`
- Modify: `Cargo.toml` (root package)

Replace the current clap derive CLI for domain commands with registry-generated commands. Keep infrastructure commands (daemon, watch, hook, hooks, status, topology) as-is.

- [ ] **Step 1: Add flotilla-commands dependency to root Cargo.toml**

- [ ] **Step 2: Restructure main.rs**

The current structure is a single clap `#[derive(Parser)]` enum. Change to:

1. Keep the top-level `Cli` struct with global args (`--repo-root`, `--config-dir`, `--socket`, `--embedded`, `--theme`)
2. Keep infrastructure subcommands (daemon, watch, status, topology, hook, hooks) as clap derive
3. Add registry-generated noun subcommands via `CliBuilder::build_noun_subcommands()`
4. Route matched noun subcommands through `CliBuilder::resolve_matches()` → `run_control_command()` or query handlers

The main dispatch becomes:
```rust
match &cli.command {
    Some(SubCommand::Daemon { .. }) => ...,      // unchanged
    Some(SubCommand::Watch { .. }) => ...,       // unchanged
    Some(SubCommand::Status { .. }) => ...,      // unchanged
    Some(SubCommand::Topology { .. }) => ...,    // unchanged
    Some(SubCommand::Hook { .. }) => ...,        // unchanged
    Some(SubCommand::Hooks { .. }) => ...,       // unchanged
    Some(SubCommand::Registry { noun, matches }) => {
        // Resolve through registry
        match cli_builder.resolve_matches(noun, matches)? {
            Resolved::DaemonCommand(cmd) => run_control_command(&cli, cmd, format).await,
            Resolved::Query(q) => run_registry_query(&cli, q, format).await,
        }
    }
    None => run_tui(cli).await,
}
```

- [ ] **Step 3: Remove old domain subcommands**

Remove the `Repo`, `Checkout`, `Host` clap derive variants, `parse_repo_command`, `parse_host_command`, `parse_host_control_command`, `RepoCommand`, `HostCommand` enums, and `normalize_cli_args`. The registry commands replace all of these.

Also remove the `Refresh` subcommand — it becomes `repo refresh` in the registry.

- [ ] **Step 4: Verify all existing CLI commands still work**

Test manually or with a script:
```bash
cargo run -- repo list
cargo run -- repo add /tmp/test-repo
cargo run -- repo remove /tmp/test-repo
cargo run -- refresh
cargo run -- checkout some-branch remove
cargo run -- host list
cargo run -- host feta status
# etc.
```

Write integration tests if feasible (the existing repo command tests in `src/main.rs` can be adapted).

- [ ] **Step 5: Run full test suite, CI checks, commit**

Run: `cargo test --workspace --locked`
Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
Commit: `feat: wire registry-generated CLI into flotilla binary`

---

### Task 5: Add shell completion support

**Files:**
- Create: `crates/flotilla-commands/src/complete.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement completion engine**

```rust
pub fn complete(registry: &Registry, line: &str, cursor_pos: usize) -> Vec<CompletionItem>;

pub struct CompletionItem {
    pub value: String,
    pub description: Option<String>,
}
```

The completion engine tokenizes the line up to cursor_pos and walks the registry:
1. Empty or first token → noun names + infrastructure subcommand names
2. Noun matched, next token → verb names for that noun (for verbs that don't need a subject) or "expecting subject"
3. Noun + subject, next token → verb names
4. Noun + verb, next tokens → arg completions from `ArgSpec`

Phase 1 completions are static (nouns, verbs, flag names). Dynamic completions (subjects from providers) come in a later phase.

- [ ] **Step 2: Add `complete` subcommand to main.rs**

```rust
/// Generate shell completions (hidden, called by shell completion scripts)
Complete {
    /// The input line
    line: String,
    /// Cursor position
    #[arg(default_value = "0")]
    cursor_pos: usize,
}
```

Output: one completion per line, tab-separated value and description.

- [ ] **Step 3: Generate shell setup scripts**

Add a `completions` subcommand that outputs the shell-specific boilerplate:
```
flotilla completions bash    # outputs bash completion script
flotilla completions zsh     # outputs zsh completion script
flotilla completions fish    # outputs fish completion script
```

Each script calls `flotilla complete "$line" $cursor_pos` and formats the output for the shell.

- [ ] **Step 4: Test completion engine**

Test cases:
- Empty line → all nouns + infrastructure commands
- `repo` → verbs for repo (add, remove, refresh)
- `repo add` → no more completions (expects path)
- `cr` → verbs for cr (open, close, link-issues)
- `check` → completes to `checkout`

- [ ] **Step 5: Run tests, commit**

Commit: `feat: shell completion engine from command registry`

---

### Task 6: Tests, cleanup, documentation

**Files:**
- Modify: `crates/flotilla-commands/src/lib.rs`
- Various test files

- [ ] **Step 1: Ensure all existing main.rs tests still pass**

The `normalize_cli_args` tests and any existing CLI integration tests need to be migrated or removed (the `--json` normalization may no longer be needed if the registry handles it uniformly).

- [ ] **Step 2: Add round-trip tests**

For each domain command that the old CLI supported, write a test that:
1. Constructs the same args the old CLI would receive
2. Passes them through the registry CLI parser
3. Asserts the same `Command` is produced

This is the key regression safety net.

- [ ] **Step 3: Run full CI suite**

Run: `cargo test --workspace --locked`
Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 4: Final commit**

Commit: `test: round-trip tests for registry CLI migration`
