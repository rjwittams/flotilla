# Hop Chain Abstraction

**Issue:** #471 (Phase A of #442, partial #368 — terminal pool attach surface only, not cloud agent teleport)
**Date:** 2026-03-23

## Problem

Terminal attach commands are built as strings through two independent, uncoordinated layers: the `TerminalPool` (which builds e.g. `cleat attach 'session' --cwd ...`) and `wrap_remote_attach_commands()` (which wraps that in `ssh -t host '...'`). Each layer does its own shell escaping. Adding a third layer (e.g. `docker exec` for environments) would make the escaping fragile and the code harder to reason about.

The attach command surface is also tightly coupled to specific transports (SSH strings hardcoded in executor code) rather than being transport-agnostic (#368).

Phase A only needs to fix the shell-backed terminal attach path used by workspace panes. The hop abstraction should become transport-agnostic at the planning/resolution layer, but the resolved output in this phase is still a shell command string because the current consumers (`WorkspaceConfig.resolved_commands`, terminal attach commands) are shell-driven. Truly non-shell transports can build on the same hop plan later, but they will need a richer resolved action than "flatten to shell".

## Design

### Core Types

```rust
/// Declarative — what needs to happen, not how
enum Hop {
    RemoteToHost { host: HostName },
    EnterEnvironment { env_id: EnvironmentId, provider: String },  // Phase C
    AttachTerminal { attachable_id: AttachableId },
    RunCommand { command: Vec<Arg> },
}

/// Hops are ordered outermost-first: the first hop is the outermost transport layer,
/// the last hop is the innermost action (e.g. AttachTerminal). Resolution walks
/// inside-out (last hop first), building up the command from the inside.
struct HopPlan(Vec<Hop>);
```

```rust
/// Structured shell command fragments for shell-backed consumers.
/// This is intentionally not a universal argv representation.
enum Arg {
    Bare(String),               // emitted verbatim at the current shell depth
    Quoted(String),             // shell-quoted at flatten time
    NestedCommand(Vec<Arg>),    // subtree rendered as a single shell-quoted argument
}
```

`Arg` is a shell-oriented tree: it models the command that will eventually be rendered for a POSIX shell consumer. `Bare` can therefore include shell syntax tokens that are intentionally emitted verbatim at the current depth (`&&`, `$SHELL`, `exec`). This keeps Phase A aligned with the actual consumers while still centralizing quoting in one place.

**Safety invariant:** `Bare` is raw shell injection at the current depth. Only resolvers (trusted code) construct `Arg` values — never from user input or untrusted external data. This invariant must be maintained by all code that creates `Arg` trees.

`NestedCommand` means "render this entire subtree into a single shell-quoted argument." It exists because some transports (SSH, `docker exec ... bash -lc`, future environment wrappers) need to pass the inner command as a single string argument to another shell layer. The per-hop resolver decides which wrapper shape to use:

- **SSH wraps via `NestedCommand`:** the inner command becomes a single string argument to SSH, potentially with further nesting for `$SHELL -l -c "..."`:
  ```
  [Bare("ssh"), Bare("-t"), Quoted("user@feta"),
    NestedCommand([Bare("$SHELL"), Bare("-l"), Bare("-c"),
      NestedCommand([Bare("cd"), Quoted("/repo"), Bare("&&"), ...inner...])])]
  ```
- **Docker exec stays shell-backed in Phase A:** the wrapper still drops into a shell and passes the inner command as a single string:
  ```
  [Bare("docker"), Bare("exec"), Bare("-it"), Quoted("abc"),
    Bare("bash"), Bare("-lc"), NestedCommand([...inner...])]
  ```

The resolver implementation knows the right structure for its transport. `flatten()` handles shell quoting only: `NestedCommand` triggers depth-aware quoting, flat args are quoted at the current depth. The tree is also useful for debug rendering — pretty-print with indentation and color-coding per nesting depth.

```rust
/// Resolved actions — what the consumer actually executes
enum ResolvedAction {
    Command(Vec<Arg>),
    SendKeys { steps: Vec<SendKeyStep> },
}

enum SendKeyStep {
    Type(String),
    WaitForPrompt,
}

struct ResolvedPlan(Vec<ResolvedAction>);
```

### Resolution: Pop, Wrap, Push

Resolution walks the hop plan inside-out. A mutable `ResolutionContext` accumulates a stack of `ResolvedAction`s:

```rust
struct ResolutionContext {
    current_host: HostName,
    current_environment: Option<EnvironmentId>,
    working_directory: Option<PathBuf>,  // set by HopPlanBuilder, read by resolvers (e.g. SSH cd prefix)
    actions: Vec<ResolvedAction>,
    nesting_depth: usize,
}
```

Each per-hop resolver takes `&mut ResolutionContext` and decides:

- **Wrap:** peek at top of action stack. If it's a `Command`, pop it, combine with own args, push the combined `Command` back. This merges two hops into one action. *How* the combination works depends on the transport — the per-hop resolver knows its own template:
  - SSH: wraps inner args in `NestedCommand` (inner becomes a single shell-string argument): `ssh -t host NestedCommand($SHELL -l -c NestedCommand(cd dir && inner))`
  - Docker exec (shell-backed Phase A shape): `docker exec -it container bash -lc NestedCommand(...inner...)`
- **SendKeys:** push a new `SendKeys` action. Creates an execution boundary — the consumer must run everything above first, then type into the resulting shell. The resolver knows the "enter" command for its transport:
  - SSH: `ssh -t user@feta` (no command arg, drops into remote shell)
  - Docker exec: `docker exec -it abc bash` (drops into container shell)
  - A subsequent hop that wants to wrap will find a `SendKeys` on top and cannot merge, so it pushes a new `Command` entry instead.
- **Collapse:** current context shows we're already at this point (e.g., `RemoteToHost(feta)` when `context.current_host == feta`). Do nothing.

N hops produce M actions where M <= N. The final stack reads top-to-bottom as execution order.

### Combine Strategy

The choice between wrap and sendkeys at each combination point is made by a `CombineStrategy` injected into the `HopResolver`:

```rust
trait CombineStrategy: Send + Sync {
    fn should_wrap(&self, hop: &Hop, context: &ResolutionContext) -> bool;
}
```

The resolver consults this strategy at each hop before deciding wrap vs sendkeys. Phase A implementations:

- **`AlwaysWrap`** — always nests commands as arguments. Matches current SSH wrapping behavior. Default.
- **`AlwaysSendKeys`** — always creates execution boundaries. Exercises the sendkeys path for testing.

Future strategies (depth-based, per-transport, capability-aware) are additional trait implementations. The plan stays pure data — it declares what needs to happen. The strategy is a runtime decision made during resolution based on current context.

In Phase A, the strategy only chooses between shell-wrapping and shell-enter-then-sendkeys. A future non-shell transport can still reuse the same `HopPlan`, but it will need an additional resolved action variant rather than going through `flatten()`.

### Per-Hop Resolvers

Each subsystem that owns a hop type provides its resolver:

```rust
trait RemoteHopResolver: Send + Sync {
    fn resolve(&self, host: &HostName, context: &mut ResolutionContext) -> Result<(), String>;
}

trait TerminalHopResolver: Send + Sync {
    fn resolve(&self, attachable_id: &AttachableId, context: &mut ResolutionContext) -> Result<(), String>;
}

// Phase C:
// trait EnvironmentHopResolver: Send + Sync { ... }
```

**`RemoteHopResolver`** — provided by the transport layer (PeerTransport or transport config). The trait implementation encapsulates all transport-specific knowledge: how to wrap (SSH uses `NestedCommand` with login shell; a future transport might use `docker exec ... bash -lc ...`), how to enter for sendkeys (SSH drops command arg), connection details (multiplex settings, host aliases). Today this knowledge lives in `remote_ssh_info()` and `wrap_remote_attach_commands()` — it migrates here. The `CombineStrategy` tells the resolver *whether* to wrap or sendkeys; the resolver knows *how* for its transport. Truly non-shell transports are a later extension point: same hop plan, richer resolved action.

**`TerminalHopResolver`** — uses the pool's new structured method (`attach_args()`) to get an `Arg` tree rather than a pre-escaped string.

### HopPlanBuilder

Constructs a `HopPlan` from the attach surface the consumer actually has:

```rust
struct HopPlanBuilder<'a> {
    attachable_store: &'a AttachableStore,
    local_host: &'a HostName,
}

impl HopPlanBuilder<'_> {
    fn build_for_attachable(&self, attachable_id: &AttachableId) -> Result<HopPlan, String>;
    fn build_for_prepared_command(&self, target_host: &HostName, command: &[Arg]) -> HopPlan;
}
```

`build_for_attachable()` is for same-host attach surfaces that really do start from an `AttachableId` (for example `TerminalManager::attach_command()` and a future `flotilla attach` implementation on a host that can resolve locally). It consults the attachable store for host affinity. If the attachable lives on a different host, prepends `RemoteToHost`. Always ends with `AttachTerminal`.

`build_for_prepared_command()` is for the existing Phase A workspace flow. `PrepareTerminalForCheckout` already returns one prepared pane command per role/count entry. The local presentation host does not have enough state to rebuild those commands from an `AttachableId`, so it builds a plan from the structured prepared command leaf instead: `[RemoteToHost(target_host), RunCommand(command.clone())]`.

Phase C adds `EnterEnvironment` hops to both entry points.

### HopResolver

Composes per-hop resolvers and drives the resolution:

```rust
struct HopResolver {
    remote: Arc<dyn RemoteHopResolver>,
    terminal: Arc<dyn TerminalHopResolver>,
    strategy: Arc<dyn CombineStrategy>,
}

impl HopResolver {
    fn resolve(&self, plan: &HopPlan, context: &mut ResolutionContext) -> Result<ResolvedPlan, String>;
}
```

Walks hops inside-out (last hop first), dispatches each to the appropriate per-hop resolver. After all hops are resolved, builds `ResolvedPlan` from `context.actions`. `RunCommand` is the leaf primitive: it pushes a `ResolvedAction::Command(command.clone())` directly. For example, given `[RemoteToHost(feta), RunCommand(cmd)]`, resolves `RunCommand` first (pushes a `Command`), then `RemoteToHost` pops and wraps it.

### Flatten

A single pure function that converts `Vec<Arg>` to a shell command string for shell-backed consumers:

```rust
fn flatten(args: &[Arg], depth: usize) -> String;
```

Walks the `Arg` tree. `Bare` values pass through. `Quoted` values get shell-quoted appropriate to the current depth. `NestedCommand` recurses at `depth + 1`. This is the only place quoting logic lives for shell-backed attach paths.

### TerminalPool Changes

Add a structured method alongside the existing string-returning one:

```rust
trait TerminalPool: Send + Sync {
    // New: returns structured args, no escaping. Sync — no pool needs I/O for arg construction.
    fn attach_args(&self, session_name: &str, command: &str,
                   cwd: &Path, env_vars: &TerminalEnvVars) -> Result<Vec<Arg>, String>;

    // Existing: becomes a default method that flattens
    async fn attach_command(&self, session_name: &str, command: &str,
                           cwd: &Path, env_vars: &TerminalEnvVars) -> Result<String, String> {
        let args = self.attach_args(session_name, command, cwd, env_vars)?;
        Ok(flatten(&args, 0))
    }

    // Other methods unchanged
}
```

Each pool implementation (cleat, shpool, passthrough) adds `attach_args()` returning `[Bare("cleat"), Bare("attach"), Quoted(session_name), ...]`. The existing `attach_command()` stays as a convenience wrapper for any callers that just need a flat string.

### Protocol / Step Data Changes

The distributed workspace flow currently serializes pane attach commands as already-flattened strings:

```rust
struct PreparedTerminalCommand {
    role: String,
    command: String,
}
```

That is exactly where the current double-escaping problem crosses the host boundary. Phase A should change this payload to carry structured shell args instead:

```rust
struct PreparedTerminalCommand {
    role: String,
    command: Vec<Arg>,
}
```

`PrepareTerminalForCheckout` on the target host produces these structured commands after allocating terminals and calling `attach_args()`. `CreateWorkspaceFromPreparedTerminal` carries them to the presentation host, which wraps them through the hop chain and flattens once at the edge when building workspace pane config.

`Arg` needs serde support since it crosses the host boundary. Use adjacently tagged representation (`{"type": "Quoted", "value": "..."}`) for debuggability — the serialized form should be readable in logs and protocol traces.

**Coordinated deployment:** changing `PreparedTerminalCommand.command` from `String` to `Vec<Arg>` means both the producing host (step 5) and consuming host (step 9) must be at the same version. In a multi-host mesh, deploy steps 4-5 and 9 together across all hosts. Acceptable in the current "no backwards compatibility" phase.

### Consumers

**Workspace pane consumer (Phase A):** the step system resolves one `HopPlan` per prepared pane command, then flattens `Command` actions to a single string for the workspace pane config. The remote `PrepareTerminalForCheckout` step produces structured shell args from `TerminalPool::attach_args()`. The local `CreateWorkspaceFromPreparedTerminal` step builds `[RemoteToHost(target_host), RunCommand(command)]`, resolves it, and flattens once. This replaces the current flow through flattened `PreparedTerminalCommand.command` strings plus `wrap_remote_attach_commands()`.

**Direct terminal attach consumer (same-host / future CLI):** `TerminalManager::attach_command()` can use `build_for_attachable()` and the hop chain internally, then flatten to string as a convenience wrapper for existing shell-based callers. This keeps the direct attach surface aligned with the workspace-pane path without pretending the current remote workspace flow starts from a local `AttachableId`.

**`flotilla attach` CLI (future, #368):** the CLI resolves the hop plan and either execs the flattened command (pure wrapping case) or drives an interactive sequence (sendkeys case). On a host that has flotilla, the resolution can collapse to `flotilla attach <id>` — the remote flotilla resolves remaining hops locally. Same plan, different resolution strategy.

### What Gets Deleted

- `wrap_remote_attach_commands()` in `executor/terminals.rs` — replaced by `RemoteHopResolver`
- `remote_ssh_info()` — knowledge moves to transport layer's resolver
- Manual shell escaping in pool `attach_command()` implementations — replaced by `attach_args()` + `flatten()`
- `escape_for_double_quotes()` usage in attach path — `flatten()` handles depth-aware quoting

## Migration

Each step keeps the system working:

1. Introduce types (`Hop`, `Arg`, `ResolvedAction`, etc.) in new `hop_chain` module
2. Implement `flatten()` with tests for depth-0, depth-1, depth-2 quoting
3. Add `attach_args()` to `TerminalPool` trait — implement for cleat, shpool, passthrough
4. Change `PreparedTerminalCommand.command` from `String` to `Vec<Arg>` in protocol + step plumbing
5. Update terminal preparation so `PrepareTerminalForCheckout` produces structured pane commands using `attach_args()` instead of flattening early
6. Build `RemoteHopResolver` — extract from `wrap_remote_attach_commands()` and `remote_ssh_info()`
7. Build `HopResolver` — compose per-hop resolvers, add direct `RunCommand` leaf handling, implement pop-wrap-push
8. Build `HopPlanBuilder` entry points for both `AttachableId` and prepared pane commands
9. Wire into step system first — `CreateWorkspaceFromPreparedTerminal` wraps structured prepared commands through the hop chain for remote terminal workspace creation (NOT `ResolveAttachCommand`, which is the cloud-agent teleport path and out of scope)
10. Wire into `TerminalManager::attach_command()` — use hop chain internally for same-host/direct attach surfaces, flatten to string
11. Delete `wrap_remote_attach_commands()` and related code

## Testing

The tree structure makes each layer independently testable:

- **`flatten()` unit tests** — depth-0/1/2 quoting, mixed Bare/Quoted/Nested, edge cases (quotes in values, spaces, special chars)
- **Per-hop resolver tests** — given config, produce expected `Vec<Arg>` tree. Pure functions, no I/O.
- **Protocol / step-data tests** — `PreparedTerminalCommand` serde round-trip with nested args, and step resolver tests proving structured pane commands survive the remote-to-local handoff without flattening.
- **`HopResolver` tests** — given plan and context, produce expected `ResolvedPlan`. Test collapse, wrapping, sendkeys boundaries, pop-wrap-push mechanics. Run the same plans through `AlwaysWrap` and `AlwaysSendKeys` strategies to verify both paths produce valid output. Include a collapse test: local-only attach where `RemoteToHost(local_host)` is a no-op.
- **`HopPlanBuilder` tests** — given attachables and hosts, produce expected `HopPlan` for `build_for_attachable()`, and verify `build_for_prepared_command()` produces the expected remote wrapper shape.
- **End-to-end flatten tests** — given a structured prepared command with known host affinity, final flattened string matches expected output (regression against old `wrap_remote_attach_commands()` behavior).
- **Snapshot tests** — pretty-printed `Arg` trees for common scenarios (local attach, remote attach, future 3-hop).
- **Debug rendering tests** — verify the tree pretty-prints readably for tracing output. Format: indented by nesting depth, `Bare` values unadorned, `Quoted` values in quotes, `NestedCommand` children indented one level. Useful for `debug!()` traces when diagnosing attach failures.

## Open Questions

- **`CloudAgentService::attach_command()`** — teleport commands (`claude --teleport`, `agent --resume`) are a separate attach path that doesn't go through `TerminalPool`. For Phase A these stay as strings — they're single commands with no nesting. Phase C may revisit if environment hops need to wrap agent attach commands.
- **Depth-aware quoting strategy** — should `flatten()` use single quotes at depth 0 and double quotes at depth 1 (matching current behavior), or adopt a uniform strategy? Need to verify compatibility with `ssh` and `docker exec` argument passing.
- **Non-shell transports** — once a transport cannot be faithfully represented as a shell command string, add a richer `ResolvedAction` variant (e.g. argv exec / delegated remote execution) instead of stretching `Arg` beyond shell-backed use.
- **`nesting_depth` on `ResolutionContext`** — used by resolvers to inform strategy decisions (e.g., prefer sendkeys over wrapping at deep nesting) and for debug rendering. Exact thresholds to be determined during implementation.
