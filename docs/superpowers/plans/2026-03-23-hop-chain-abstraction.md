# Hop Chain Abstraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace string-based terminal attach command construction with a structured hop chain that centralizes shell quoting and supports late-binding resolution strategies.

**Architecture:** A new `hop_chain` module defines `Arg` trees (shell command fragments), `Hop` plans (declarative intent), and a resolver that walks plans inside-out, composing commands via pop-wrap-push. Per-hop resolvers (SSH, terminal pool) encapsulate transport-specific knowledge. The workspace pane flow is the primary consumer in Phase A.

**Tech Stack:** Rust, serde (adjacently tagged for `Arg`), insta (snapshot tests for `Arg` trees)

**Spec:** `docs/superpowers/specs/2026-03-23-hop-chain-abstraction-design.md`

---

## File Structure

### Dependency note

`Arg` and `ResolvedPaneCommand` must live in `flotilla-protocol` (not `flotilla-core`) because `ResolvedPaneCommand` crosses the wire and `flotilla-protocol` cannot depend on `flotilla-core`. The hop chain module in `flotilla-core` re-exports `Arg` from the protocol crate. All other hop chain types (`Hop`, `HopPlan`, `ResolutionContext`, resolvers, etc.) live in `flotilla-core` since they don't cross the wire.

### New files

| File | Responsibility |
|------|---------------|
| `crates/flotilla-protocol/src/arg.rs` | `Arg` enum (Literal, Quoted, NestedCommand) + `flatten()` — wire-serializable, used by both crates |
| `crates/flotilla-core/src/hop_chain/mod.rs` | Re-exports `Arg` from protocol. Defines `Hop`, `HopPlan`, `ResolvedAction`, `SendKeyStep`, `ResolvedPlan`, `ResolutionContext` |
| `crates/flotilla-core/src/hop_chain/flatten.rs` | Re-exports `flatten()` from protocol, or adds hop-chain-specific rendering helpers |
| `crates/flotilla-core/src/hop_chain/resolver.rs` | `HopResolver`, `CombineStrategy` trait, `AlwaysWrap`, `AlwaysSendKeys` |
| `crates/flotilla-core/src/hop_chain/remote.rs` | `RemoteHopResolver` trait + `SshRemoteHopResolver` impl |
| `crates/flotilla-core/src/hop_chain/terminal.rs` | `TerminalHopResolver` trait + `PoolTerminalHopResolver` impl |
| `crates/flotilla-core/src/hop_chain/builder.rs` | `HopPlanBuilder` — `build_for_attachable()` + `build_for_prepared_command()` |
| `crates/flotilla-core/src/hop_chain/tests.rs` | Unit tests for all hop chain components |

### Modified files

| File | Change |
|------|--------|
| `crates/flotilla-core/src/lib.rs` | Add `pub mod hop_chain;` |
| `crates/flotilla-core/src/providers/terminal/mod.rs:27-32` | Add `attach_args()` to `TerminalPool` trait, make `attach_command()` a default method |
| `crates/flotilla-core/src/providers/terminal/cleat.rs:66-89` | Add `attach_args()` returning structured `Vec<Arg>` |
| `crates/flotilla-core/src/providers/terminal/shpool.rs:484-514` | Add `attach_args()` returning structured `Vec<Arg>` |
| `crates/flotilla-core/src/providers/terminal/passthrough.rs:17-30` | Add `attach_args()` returning structured `Vec<Arg>` |
| `crates/flotilla-protocol/src/commands.rs:27-30` | Add `ResolvedPaneCommand` struct alongside `PreparedTerminalCommand` |
| `crates/flotilla-protocol/src/commands.rs:188-196` | Update `CommandValue::TerminalPrepared` to carry `Vec<ResolvedPaneCommand>` |
| `crates/flotilla-core/src/step.rs:83-96` | Update `CreateWorkspaceFromPreparedTerminal` to carry `Vec<ResolvedPaneCommand>` |
| `crates/flotilla-core/src/executor/terminals.rs:80-137` | Update `prepare_terminal_commands()` to produce `ResolvedPaneCommand`s |
| `crates/flotilla-core/src/executor/workspace.rs:81-112` | Replace `wrap_remote_attach_commands()` with hop chain resolution |
| `crates/flotilla-core/src/executor.rs:464-534` | Update step resolver for new types |
| `crates/flotilla-core/src/terminal_manager.rs:117-132` | Use hop chain internally in `attach_command()` |

### Deleted code (final task)

| File | What |
|------|------|
| `crates/flotilla-core/src/executor/terminals.rs:173-250` | `wrap_remote_attach_commands()`, `remote_ssh_info()`, `RemoteSshInfo`, `shell_quote()`, `escape_for_double_quotes()` |

---

### Task 1: Core types — Arg (in protocol), Hop/HopPlan/ResolvedAction (in core)

**Files:**
- Create: `crates/flotilla-protocol/src/arg.rs`
- Modify: `crates/flotilla-protocol/src/lib.rs`
- Create: `crates/flotilla-core/src/hop_chain/mod.rs`
- Modify: `crates/flotilla-core/src/lib.rs`

- [ ] **Step 1: Create Arg in flotilla-protocol**

`Arg` lives in protocol because `ResolvedPaneCommand` (which contains `Vec<Arg>`) crosses the wire. Protocol cannot depend on core.

```rust
// crates/flotilla-protocol/src/arg.rs
use serde::{Deserialize, Serialize};

/// Structured shell command fragments for shell-backed consumers.
/// This is intentionally not a universal argv representation.
///
/// **Safety invariant:** `Literal` is raw shell at the current depth.
/// Only resolvers (trusted code) construct `Arg` values. When serialized
/// across the wire, this extends to a protocol-level trust assumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Arg {
    /// Emitted verbatim at the current shell depth (flags, syntax tokens, expansion vars).
    Literal(String),
    /// Shell-quoted at flatten time (single-quoted, no expansion).
    Quoted(String),
    /// Subtree rendered as a single shell-quoted argument at the next depth.
    NestedCommand(Vec<Arg>),
}
```

Register in `crates/flotilla-protocol/src/lib.rs`: `pub mod arg;`

- [ ] **Step 2: Add serde round-trip test for Arg**

In a test module in `arg.rs`, verify `Arg` with nested variants survives serde_json round-trip with the adjacently tagged format.

- [ ] **Step 3: Create hop_chain module in flotilla-core**

```rust
// crates/flotilla-core/src/hop_chain/mod.rs
pub mod flatten;
pub mod resolver;
pub mod remote;
pub mod terminal;
pub mod builder;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

pub use flotilla_protocol::arg::Arg;
use crate::attachable::AttachableId;
use flotilla_protocol::HostName;

/// Declarative — what needs to happen, not how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hop {
    RemoteToHost { host: HostName },
    AttachTerminal { attachable_id: AttachableId },
    RunCommand { command: Vec<Arg> },
}

/// Hops are ordered outermost-first: the first hop is the outermost transport
/// layer, the last hop is the innermost action. Resolution walks inside-out.
#[derive(Debug, Clone)]
pub struct HopPlan(pub Vec<Hop>);

/// What the consumer actually executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedAction {
    Command(Vec<Arg>),
    SendKeys { steps: Vec<SendKeyStep> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendKeyStep {
    Type(String),
    WaitForPrompt,
}

/// The resolved output — M actions from N hops where M <= N.
#[derive(Debug, Clone)]
pub struct ResolvedPlan(pub Vec<ResolvedAction>);

/// Mutable state accumulated during inside-out resolution.
pub struct ResolutionContext {
    pub current_host: HostName,
    pub current_environment: Option<String>,  // placeholder, becomes EnvironmentId in Phase C
    pub working_directory: Option<PathBuf>,
    pub actions: Vec<ResolvedAction>,
    pub nesting_depth: usize,
}
```

- [ ] **Step 2: Register the module**

Add to `crates/flotilla-core/src/lib.rs`:
```rust
pub mod hop_chain;
```

- [ ] **Step 3: Create empty submodule files**

Create empty files with just `// TODO` for each submodule:
- `crates/flotilla-core/src/hop_chain/flatten.rs`
- `crates/flotilla-core/src/hop_chain/resolver.rs`
- `crates/flotilla-core/src/hop_chain/remote.rs`
- `crates/flotilla-core/src/hop_chain/terminal.rs`
- `crates/flotilla-core/src/hop_chain/builder.rs`
- `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p flotilla-core`
Expected: compiles with no errors (unused warnings OK)

- [ ] **Step 5: Commit**

```
feat: introduce hop chain core types (Arg, Hop, HopPlan, ResolvedAction)
```

---

### Task 2: flatten() — render Arg trees to shell strings

**Files:**
- Modify: `crates/flotilla-protocol/src/arg.rs` (flatten lives alongside Arg since it's a pure function on the type)
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs` (or `crates/flotilla-protocol` tests)

- [ ] **Step 1: Write failing tests for flatten**

Key test cases in `tests.rs`:
1. Single `Literal` — passes through verbatim
2. Single `Quoted` — single-quoted with `'\''` escaping for embedded quotes
3. `NestedCommand` at depth 0 — subtree rendered and single-quoted as one arg
4. Nested two levels deep — depth 0 single-quotes outer, depth 1 uses appropriate escaping inside the single-quoted string
5. Mixed args — `[Literal("ssh"), Literal("-t"), Quoted("user@feta"), NestedCommand([Literal("cleat"), Quoted("sess")])]`
6. Empty NestedCommand — `NestedCommand(vec![])` produces `''`
7. Quoted value containing single quotes, spaces, special chars
8. Regression test: produce the same output as current `wrap_remote_attach_commands()` for a known input (read the current code's escaping pattern and verify flatten matches)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-core hop_chain`
Expected: compilation errors (flatten not defined)

- [ ] **Step 3: Implement flatten**

```rust
// crates/flotilla-protocol/src/arg.rs (alongside Arg definition)
use super::Arg; // or just `use crate::arg::Arg;`

/// Render a Vec<Arg> to a shell command string.
/// Literal values pass through verbatim. Quoted values are single-quoted.
/// NestedCommand subtrees are recursively flattened and the result is
/// single-quoted as a single argument.
pub fn flatten(args: &[Arg], depth: usize) -> String {
    args.iter()
        .map(|arg| match arg {
            Arg::Literal(s) => s.clone(),
            Arg::Quoted(s) => shell_quote(s),
            Arg::NestedCommand(inner) => {
                let rendered = flatten(inner, depth + 1);
                shell_quote(&rendered)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
```

Note: this initial implementation uses single-quoting at all depths. The spec's open question about depth-aware quoting (single at depth 0, double at depth 1 for `$SHELL` expansion) will be validated by the regression test in step 1. If the regression test fails, `flatten()` needs depth-aware quoting — revisit at that point.

- [ ] **Step 4: Add Display impl for debug rendering**

```rust
use std::fmt;

impl fmt::Display for Arg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Arg::Literal(s) => write!(f, "{s}"),
            Arg::Quoted(s) => write!(f, "\"{s}\""),
            Arg::NestedCommand(inner) => {
                writeln!(f, "NestedCommand(")?;
                for arg in inner {
                    writeln!(f, "  {arg}")?;
                }
                write!(f, ")")
            }
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p flotilla-core hop_chain`
Expected: all flatten tests pass

- [ ] **Step 6: Commit**

```
feat: implement flatten() for Arg trees with depth-aware shell quoting
```

---

### Task 3: attach_args() on TerminalPool trait + implementations

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/mod.rs:27-32`
- Modify: `crates/flotilla-core/src/providers/terminal/cleat.rs:66-89`
- Modify: `crates/flotilla-core/src/providers/terminal/shpool.rs:484-514`
- Modify: `crates/flotilla-core/src/providers/terminal/passthrough.rs:17-30`
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 1: Write failing tests**

Test each pool's `attach_args()` produces the expected `Arg` tree. For cleat:
```rust
#[test]
fn cleat_attach_args_structure() {
    let pool = CleatTerminalPool::new("cleat");
    let args = pool.attach_args("sess-1", "bash", Path::new("/repo"), &vec![
        ("FLOTILLA_ATTACHABLE_ID".into(), "att-xyz".into()),
    ]).unwrap();
    // Assert the Arg tree structure — don't test the flattened string,
    // test the tree shape. Use insta::assert_debug_snapshot! for readability.
}
```

- [ ] **Step 2: Add attach_args() to TerminalPool trait**

In `crates/flotilla-core/src/providers/terminal/mod.rs`, add to the trait:
```rust
fn attach_args(&self, session_name: &str, command: &str, cwd: &Path, env_vars: &TerminalEnvVars) -> Result<Vec<Arg>, String>;
```

Make `attach_command()` a default method that calls `attach_args()` + `flatten()`.

- [ ] **Step 3: Implement attach_args() for CleatTerminalPool**

Port the logic from the existing `attach_command()` but return `Vec<Arg>` instead of a string. Use `Literal` for flags/subcommands, `Quoted` for values (session name, cwd, command string).

- [ ] **Step 4: Implement attach_args() for ShpoolTerminalPool**

Same pattern — structured `Vec<Arg>` from the existing shpool attach logic.

- [ ] **Step 5: Implement attach_args() for PassthroughTerminalPool**

Simplest — returns the command as `Literal` or prepends env vars.

- [ ] **Step 6: Run tests**

Run: `cargo test -p flotilla-core`
Expected: all existing tests still pass (attach_command default method produces same strings), new attach_args tests pass.

- [ ] **Step 7: Run CI checks**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings && cargo +nightly-2026-03-12 fmt --check`

- [ ] **Step 8: Commit**

```
feat: add attach_args() to TerminalPool trait with structured Arg output
```

---

### Task 4: ResolvedPaneCommand in protocol + step plumbing

**Files:**
- Modify: `crates/flotilla-protocol/src/commands.rs:27-30, 188-196`
- Modify: `crates/flotilla-core/src/step.rs:83-96`

- [ ] **Step 1: Add ResolvedPaneCommand to protocol**

In `crates/flotilla-protocol/src/commands.rs`:
```rust
/// Structured resolved attach command for a workspace pane.
/// Produced on the target host, consumed on the presentation host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPaneCommand {
    pub role: String,
    pub args: Vec<Arg>,
}
```

Note: `Arg` lives in `flotilla-core`, not `flotilla-protocol`. Either move `Arg` to protocol (it's a serialization type) or re-export. Check whether `flotilla-protocol` can depend on `flotilla-core` or if the dependency goes the other way — if protocol cannot depend on core, `Arg` must live in protocol.

- [ ] **Step 2: Update CommandValue::TerminalPrepared**

Add a variant or field to carry `Vec<ResolvedPaneCommand>` alongside the existing `Vec<PreparedTerminalCommand>`. The existing variant carries string commands; the new data carries structured args. During migration, the step resolver can use whichever is populated.

- [ ] **Step 3: Update CreateWorkspaceFromPreparedTerminal step action**

In `crates/flotilla-core/src/step.rs`, update the variant to accept `Vec<ResolvedPaneCommand>`:
```rust
CreateWorkspaceFromPreparedTerminal {
    target_host: HostName,
    branch: String,
    checkout_path: PathBuf,
    commands: Vec<ResolvedPaneCommand>,  // was Vec<PreparedTerminalCommand>
}
```

- [ ] **Step 4: Fix compilation errors from type changes**

Update all match arms, constructors, and consumers of the changed types. The executor step resolver (`executor.rs:464-534`) and the plan builder will need updating.

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace --locked`
Expected: all pass (types changed but no behavior change yet)

- [ ] **Step 6: Commit**

```
feat: add ResolvedPaneCommand with structured Arg payload
```

---

### Task 5: Terminal preparation produces ResolvedPaneCommand

**Files:**
- Modify: `crates/flotilla-core/src/executor/terminals.rs:80-137`
- Modify: `crates/flotilla-core/src/executor.rs:495-534`

- [ ] **Step 1: Update prepare_terminal_commands to return ResolvedPaneCommands**

In `TerminalPreparationService::prepare_terminal_commands()`, change the success path to call `attach_args()` instead of `attach_command()`, producing `ResolvedPaneCommand`s. The fallback path (when allocation fails) wraps the original string command in `Arg::Literal`.

- [ ] **Step 2: Update PrepareTerminalForCheckout step handler**

In `executor.rs`, update the handler for `PrepareTerminalForCheckout` to produce `CommandValue::TerminalPrepared` with the new `ResolvedPaneCommand` data.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace --locked`

- [ ] **Step 4: Commit**

```
feat: terminal preparation produces structured ResolvedPaneCommand
```

Deploy atomically with Task 4 across mesh hosts.

---

### Task 6: CombineStrategy trait + AlwaysWrap + AlwaysSendKeys

**Files:**
- Modify: `crates/flotilla-core/src/hop_chain/resolver.rs`
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 1: Write tests for both strategies**

```rust
#[test]
fn always_wrap_returns_true() {
    let strategy = AlwaysWrap;
    let hop = Hop::RemoteToHost { host: HostName::new("feta") };
    let context = test_context("kiwi");
    assert!(strategy.should_wrap(&hop, &context));
}

#[test]
fn always_send_keys_returns_false() {
    let strategy = AlwaysSendKeys;
    // same setup
    assert!(!strategy.should_wrap(&hop, &context));
}
```

- [ ] **Step 2: Implement CombineStrategy trait and both impls**

```rust
// crates/flotilla-core/src/hop_chain/resolver.rs
use super::*;

pub trait CombineStrategy: Send + Sync {
    fn should_wrap(&self, hop: &Hop, context: &ResolutionContext) -> bool;
}

pub struct AlwaysWrap;
impl CombineStrategy for AlwaysWrap {
    fn should_wrap(&self, _hop: &Hop, _context: &ResolutionContext) -> bool { true }
}

pub struct AlwaysSendKeys;
impl CombineStrategy for AlwaysSendKeys {
    fn should_wrap(&self, _hop: &Hop, _context: &ResolutionContext) -> bool { false }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p flotilla-core hop_chain`

- [ ] **Step 4: Commit**

```
feat: add CombineStrategy trait with AlwaysWrap and AlwaysSendKeys
```

---

### Task 7: RemoteHopResolver — extract SSH wrapping knowledge

**Files:**
- Modify: `crates/flotilla-core/src/hop_chain/remote.rs`
- Read: `crates/flotilla-core/src/executor/terminals.rs:173-250` (source of SSH knowledge)
- Read: `crates/flotilla-core/src/config.rs:213-275` (hosts config)
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 1: Write tests for SSH resolver wrap and sendkeys**

Test the `SshRemoteHopResolver` with known SSH config:
1. Wrap case: given inner Command on stack, produces SSH-wrapped Command with `NestedCommand`
2. SendKeys case: produces `Command(ssh -t host)` + leaves inner on stack
3. Collapse case: target host == current host → no-op
4. Multiplex args present when configured
5. Working directory produces `cd <dir> &&` prefix inside the nested command

- [ ] **Step 2: Define RemoteHopResolver trait**

```rust
pub trait RemoteHopResolver: Send + Sync {
    fn resolve_wrap(&self, host: &HostName, context: &mut ResolutionContext) -> Result<(), String>;
    fn resolve_enter(&self, host: &HostName, context: &mut ResolutionContext) -> Result<(), String>;
}
```

**Spec departure (intentional):** The spec defines a single `resolve()` method. This plan splits it into `resolve_wrap` and `resolve_enter` so the `HopResolver` (which owns the `CombineStrategy`) makes the wrap-vs-sendkeys decision and dispatches to the appropriate method. The resolver impl just needs to know *how* to do each, not *whether* to.

- [ ] **Step 3: Implement SshRemoteHopResolver**

Extract SSH knowledge from `wrap_remote_attach_commands()` and `remote_ssh_info()`:
- Reads hosts config for target host (SSH user, multiplex settings)
- `resolve_wrap`: pops inner Command, wraps in `NestedCommand` with `$SHELL -l -c` and optional `cd <dir> &&` prefix from `context.working_directory`
- `resolve_enter`: pushes `Command([Literal("ssh"), Literal("-t"), ...multiplex..., Quoted(target)])`

- [ ] **Step 4: Run tests**

Run: `cargo test -p flotilla-core hop_chain`

- [ ] **Step 5: Commit**

```
feat: add SshRemoteHopResolver extracting SSH knowledge from executor
```

---

### Task 8: TerminalHopResolver + PoolTerminalHopResolver

**Files:**
- Modify: `crates/flotilla-core/src/hop_chain/terminal.rs`
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 1: Write tests**

Test that `PoolTerminalHopResolver` pushes the right `Command` from pool's `attach_args()`.

- [ ] **Step 2: Define TerminalHopResolver trait and impl**

```rust
pub trait TerminalHopResolver: Send + Sync {
    fn resolve(&self, attachable_id: &AttachableId, context: &mut ResolutionContext) -> Result<(), String>;
}

pub struct PoolTerminalHopResolver { /* terminal_manager or pool + store */ }
```

The resolver looks up the attachable in the store, calls `pool.attach_args()`, pushes the result as `ResolvedAction::Command`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p flotilla-core hop_chain`

- [ ] **Step 4: Commit**

```
feat: add PoolTerminalHopResolver using attach_args()
```

---

### Task 9: HopResolver — compose per-hop resolvers with pop-wrap-push

**Files:**
- Modify: `crates/flotilla-core/src/hop_chain/resolver.rs`
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 1: Write tests for the full resolution algorithm**

Key scenarios:
1. `[RemoteToHost(feta), RunCommand(cmd)]` with AlwaysWrap → 1 action (SSH wraps RunCommand)
2. `[RemoteToHost(feta), RunCommand(cmd)]` with AlwaysSendKeys → 2 actions (SSH enter + SendKeys)
3. `[RemoteToHost(local_host), RunCommand(cmd)]` → collapse, 1 action (just RunCommand)
4. `[RemoteToHost(feta), AttachTerminal(id)]` with AlwaysWrap → 1 action (SSH wraps terminal attach)
5. Snapshot tests for the resulting `Arg` trees

- [ ] **Step 2: Implement HopResolver::resolve()**

```rust
pub struct HopResolver {
    pub remote: Arc<dyn RemoteHopResolver>,
    pub terminal: Arc<dyn TerminalHopResolver>,
    pub strategy: Arc<dyn CombineStrategy>,
}

impl HopResolver {
    pub fn resolve(&self, plan: &HopPlan, context: &mut ResolutionContext) -> Result<ResolvedPlan, String> {
        // Walk inside-out (reverse order)
        for hop in plan.0.iter().rev() {
            match hop {
                Hop::RunCommand { command } => {
                    context.actions.push(ResolvedAction::Command(command.clone()));
                }
                Hop::AttachTerminal { attachable_id } => {
                    self.terminal.resolve(attachable_id, context)?;
                }
                Hop::RemoteToHost { host } => {
                    if host == &context.current_host {
                        continue; // collapse
                    }
                    if self.strategy.should_wrap(hop, context) {
                        self.remote.resolve_wrap(host, context)?;
                    } else {
                        self.remote.resolve_enter(host, context)?;
                    }
                    context.nesting_depth += 1;
                }
            }
        }
        Ok(ResolvedPlan(std::mem::take(&mut context.actions)))
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p flotilla-core hop_chain`

- [ ] **Step 4: Commit**

```
feat: implement HopResolver with pop-wrap-push resolution algorithm
```

---

### Task 10: HopPlanBuilder

**Files:**
- Modify: `crates/flotilla-core/src/hop_chain/builder.rs`
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 1: Write tests**

1. `build_for_attachable` — local attachable → `[AttachTerminal(id)]`
2. `build_for_attachable` — remote attachable → `[RemoteToHost(host), AttachTerminal(id)]`
3. `build_for_prepared_command` — always `[RemoteToHost(target), RunCommand(cmd)]`

- [ ] **Step 2: Implement HopPlanBuilder**

```rust
pub struct HopPlanBuilder<'a> {
    attachable_store: &'a AttachableStore,
    local_host: &'a HostName,
}

impl HopPlanBuilder<'_> {
    pub fn build_for_attachable(&self, attachable_id: &AttachableId) -> Result<HopPlan, String> {
        // Look up attachable, check host affinity
        // If remote, prepend RemoteToHost
        // Always end with AttachTerminal
    }

    pub fn build_for_prepared_command(&self, target_host: &HostName, command: &[Arg]) -> HopPlan {
        let mut hops = Vec::new();
        if target_host != self.local_host {
            hops.push(Hop::RemoteToHost { host: target_host.clone() });
        }
        hops.push(Hop::RunCommand { command: command.to_vec() });
        HopPlan(hops)
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p flotilla-core hop_chain`

- [ ] **Step 4: Commit**

```
feat: add HopPlanBuilder with attachable and prepared-command entry points
```

---

### Task 11: Wire into CreateWorkspaceFromPreparedTerminal

**Files:**
- Modify: `crates/flotilla-core/src/executor/workspace.rs:81-112`
- Modify: `crates/flotilla-core/src/executor.rs:464-479`
- Modify: `crates/flotilla-core/src/executor/tests.rs`

This is the key integration point — the workspace pane flow uses the hop chain.

- [ ] **Step 1: Write/update integration test**

Test that `CreateWorkspaceFromPreparedTerminal` with a remote target host produces the same workspace pane commands as the old `wrap_remote_attach_commands()` flow. Use a known SSH config and verify the flattened output matches.

- [ ] **Step 2: Update create_workspace_from_prepared_terminal**

Replace the call to `wrap_remote_attach_commands()` with:
1. For each `ResolvedPaneCommand`, build a `HopPlan` via `build_for_prepared_command(target_host, &cmd.args)`
2. Create `ResolutionContext` with `working_directory` from `checkout_path`
3. Resolve via `HopResolver` with `AlwaysWrap`
4. Flatten the resulting `ResolvedAction::Command` to a string
5. Pass to workspace manager as before

- [ ] **Step 3: Update step resolver**

In `executor.rs`, update the `CreateWorkspaceFromPreparedTerminal` handler to pass `Vec<ResolvedPaneCommand>` to the workspace orchestrator.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace --locked`

- [ ] **Step 5: Run CI checks**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings && cargo +nightly-2026-03-12 fmt --check`

- [ ] **Step 6: Commit**

```
feat: wire hop chain into workspace pane creation flow
```

---

### Task 12: Wire into TerminalManager::attach_command

**Files:**
- Modify: `crates/flotilla-core/src/terminal_manager.rs:117-132`

- [ ] **Step 1: Update attach_command to use hop chain internally**

Use `build_for_attachable()` + `HopResolver` + `flatten()` internally. The method still returns `Result<String, String>` for existing callers.

- [ ] **Step 2: Run tests**

Run: `cargo test --workspace --locked`

- [ ] **Step 3: Commit**

```
feat: use hop chain internally in TerminalManager::attach_command
```

---

### Task 13: Delete old wrapping code

**Files:**
- Modify: `crates/flotilla-core/src/executor/terminals.rs`
- Modify: `crates/flotilla-core/src/executor/tests.rs` (remove/migrate tests that directly call deleted functions)

- [ ] **Step 1: Delete wrap_remote_attach_commands and related functions**

Remove:
- `wrap_remote_attach_commands()` (lines 173-211)
- `remote_ssh_info()` (lines 218-232)
- `RemoteSshInfo` struct (lines 213-216)
- `shell_quote()` (lines 234-236)
- `escape_for_double_quotes()` (lines 238-250)

- [ ] **Step 2: Fix any remaining references**

Search for callers of deleted functions. All should have been migrated in Tasks 11-12.

- [ ] **Step 3: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo +nightly-2026-03-12 fmt --check`

- [ ] **Step 4: Commit**

```
refactor: delete wrap_remote_attach_commands and legacy shell escaping
```

---

### Task 14: Snapshot tests + final verification

**Files:**
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`

- [ ] **Step 1: Add insta snapshot tests for common Arg trees**

Scenarios:
1. Local terminal attach (no remote hop)
2. Remote terminal attach via SSH with AlwaysWrap
3. Remote terminal attach via SSH with AlwaysSendKeys
4. Collapse case (target == local)
5. Debug rendering (Display impl output)

- [ ] **Step 2: Add end-to-end regression test**

Build a full workspace-creation scenario with known inputs, verify the flattened pane command strings match expected output.

- [ ] **Step 3: Run all tests**

Run: `cargo test --workspace --locked`

- [ ] **Step 4: Accept snapshots**

Run: `cargo insta review` — verify each snapshot is correct (not blindly accepting).

- [ ] **Step 5: Final CI run**

Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`

- [ ] **Step 6: Commit**

```
test: add snapshot tests and end-to-end regression for hop chain
```
