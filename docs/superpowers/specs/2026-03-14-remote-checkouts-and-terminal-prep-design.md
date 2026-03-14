# Remote Checkouts And Terminal Prep Design

## Summary

This batch covers the next multi-host execution step after remote command routing: remote checkout creation (`#274`) plus the first phase of remote terminal execution (`#273`).

The design deliberately keeps the "presentation host" local to the TUI process. Remote hosts execute commands and prepare terminal attach information, but local workspace managers remain responsible for creating panes/tabs visible to the user.

Session handoff (`#275`) stays out of scope.

## Scope

### In scope

- Add app-level target host state, surfaced in the status UI and applied consistently to host-executed commands
- Route checkout creation to the selected remote host using existing remote command routing
- Allow remote-created checkouts to replicate back through normal provider-data exchange
- Add phase-one remote terminal preparation for passthrough terminal pools
- Thread remote terminal preparation through local workspace creation without redefining workspaces as remote objects

### Out of scope

- Session handoff / migration (`#275`)
- Remote workspace manager ownership or cross-host workspace persistence
- Rich host picker UI, override dialogs, or automatic host selection policy
- Shpool-backed remote terminal preparation beyond the contract needed to support it later

## Design Principles

### Presentation host remains local

The machine running the TUI owns presentation. Workspace creation, pane layout, and "what the user sees" remain local concerns even when the underlying shell or long-lived process runs remotely.

This keeps the model compatible with:

- users running the TUI in a separate terminal from their preferred multiplexer
- users with more than one active machine side-by-side
- heterogeneous setups where the local presentation host and the target execution host have different workspace managers

### Remote hosts execute, local host presents

Remote hosts execute checkout creation and terminal preparation using their own providers and filesystem. The local host receives results and turns them into presentation-local workspace-manager commands.

### Reuse existing command routing

The existing `Command.host` routing introduced by remote command routing should remain the mechanism for remote execution. This batch should avoid inventing a second "remote action" transport.

## Host Targeting

Add a single app-level target host selector.

### Behavior

- Default target host is the local host
- The target host is shown in the status bar
- `h` and status-bar click cycle through known hosts
- The target host becomes the default execution host for host-executed commands

### Command application rule

Commands fall into three categories:

1. Selector-targeted commands
   - These use the currently selected target host unless some stronger host identity applies
   - Example: checkout creation

2. Item-fixed commands
   - These derive host from the selected work item or repo model and should not be overridden by the selector
   - Example: an action invoked directly on an already-remote checkout item

3. Always-local presentation commands
   - These are tied to the machine running the TUI and must not be routed remotely
   - Examples: file picker flows, local workspace switching, similar presentation-only actions

The target host is therefore a universal default, not a checkout-only special case.

## Remote Checkout Creation (`#274`)

### User flow

1. User selects target host from the status bar
2. User triggers checkout creation as normal
3. TUI executor stamps `Command.host = Some(target_host)` when the target is remote
4. Existing remote command routing forwards `CommandAction::Checkout` to the remote daemon
5. Remote daemon executes the checkout using its local `CheckoutManager`
6. The new remote checkout appears back on the local presentation host via normal replicated provider data

### Protocol and command model

No new protocol command type is required for checkout creation.

The batch should continue using:

- `Command`
- `Command.host`
- `CommandAction::Checkout`

This keeps checkout creation aligned with the existing remote-command substrate and reduces new protocol surface.

### Failure behavior

- If the chosen target host is unknown or disconnected, fail early in the TUI with a clear status message
- If remote execution fails, preserve the returned `CommandResult::Error` path and surface it through the existing lifecycle event flow
- No workspace creation should be chained automatically after checkout creation in this batch

## Remote Terminal Preparation (`#273`, Phase A)

### Problem framing

A workspace contains multiple terminal/pane entries. For remote execution, the target host must decide how a terminal should be prepared in its own environment, while the local presentation host still owns the actual workspace/pane creation.

### Core model

Remote terminal execution is a two-stage prepare/attach flow:

1. Remote host prepares terminal execution for a pane intent
2. Remote host returns attach information
3. Local workspace manager uses that attach information as a local command when building the workspace

### Passthrough phase

For passthrough terminal pools:

- the remote side does not create a durable managed session
- instead it returns the command that should be executed on the remote host for that pane intent
- the local presentation host wraps that command in the appropriate SSH/multiplexed attach path and gives it to the local workspace manager

This gets the end-to-end model working without requiring remote terminal persistence first.

### Why not remote workspace creation

Workspaces are primarily presentation constructs. Treating them as remote-owned objects would force premature decisions about mixed local/remote workspace-manager ownership, heterogeneous multiplexer environments, and persistence semantics. This batch should avoid that.

## Terminal Attach Contract

The critical design requirement is that attach behavior must be derived from the target host's own providers, not guessed locally.

### Rule

The target host returns attach/preparation data that is valid in its own environment. The local presentation host merely embeds that into a locally executed workspace-manager command.

This avoids problems such as:

- provider path differences across hosts
- different provider capabilities across hosts
- differing shell assumptions or command locations

### Shape

This batch should introduce a terminal-preparation result contract, not reuse workspace-manager results directly.

Conceptually, the remote response should describe:

- which checkout/session context the pane belongs to
- what local command should be executed by the presentation host to reach the target host and attach or run the desired terminal behavior
- whether the remote side actually created durable terminal state or only returned a passthrough command

The exact Rust type names can be chosen during implementation, but the contract should remain explicitly terminal-oriented rather than workspace-oriented.

## Workspace Creation Integration

`CreateWorkspaceForCheckout` remains the orchestration point for workspace creation.

### For this batch

- read the local workspace template as today
- for each pane/terminal entry that targets a remote host, request remote terminal preparation
- wait for the returned attach information
- pass the resolved local commands into the local workspace manager

### Why not batch over the wire yet

A single batch "prepare workspace terminals" protocol message is plausible, but unnecessary for the first pass.

Per-pane preparation is preferred initially because:

- it keeps the new terminal-prep contract smaller
- it avoids prematurely encoding workspace structure into peer protocol
- it is easier to reason about while passthrough behavior is still being proven

If the per-pane approach proves too chatty or awkward, a later batch can introduce a batched prepare request.

## Execution Sequencing

Recommended order:

1. Add target-host state and status-bar toggle/cycle behavior
2. Apply universal host-targeting rules in the TUI executor
3. Implement remote checkout creation using existing remote command routing
4. Add daemon/integration coverage proving remote-created checkouts replicate back correctly
5. Add terminal preparation contract and passthrough phase
6. Integrate terminal preparation into local workspace creation
7. Reassess shpool-specific preparation as the next increment

## Testing Strategy

### TUI / executor

- target host selector defaults to local
- `h` / status UI toggles through known hosts
- selector-targeted commands stamp `Command.host`
- item-fixed actions preserve item host
- presentation-local actions remain local regardless of selector

### Daemon / routing

- remote checkout commands are forwarded to the target host
- remote daemon executes `CommandAction::Checkout` locally
- remote lifecycle events route back to the requester

### Integration

- remote-created checkout replicates back through normal snapshot flow
- replicated checkout is attributed to the target host
- terminal-preparation passthrough returns attach info that local workspace creation consumes

### Determinism

Reuse the current deterministic discovery/test-support patterns. Terminal preparation tests should avoid real host tools and rely on explicit fake provider behavior.

## Future Follow-ons

- richer host selection UI and overrides
- automatic host policy / AI-assisted defaulting
- shpool-specific remote terminal preparation
- session handoff built on top of remote checkout creation plus terminal preparation
