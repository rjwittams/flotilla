# Service Host Resolution

**Status:** Exploratory — captures architectural direction. Not implementation-ready.
**Related:** #465 (Provider vs Service), #256 (log-based replication), #530 (Docker environments E2E)

## Problem

Steps carry a concrete `HostName` chosen at plan-build time. This creates three problems:

1. **Wrong host bugs.** `build_plan` must choose the right host for every step. When it guesses wrong (e.g. using `local_host` instead of `target_host` for `RemoveCheckout`), the step fails with "not found" errors. Each new command is an opportunity for the same mistake.

2. **Ambient assumptions.** "GitHub API calls happen on the first hop" works today because the first hop usually has the token. Nothing enforces this. A mesh topology where the client daemon lacks GitHub credentials breaks silently.

3. **`HostName::local()` as semantic default.** The codebase uses the local machine's hostname as a fallback throughout the execution pipeline — in `refresh.rs`, in `build_plan`, in `TerminalManager` construction. This conflates "I am this host" with "this step should run here."

## Core Idea

Steps express *what they need*, not *where to run*. The mesh resolves service instances to hosts at dispatch time.

The planner emits symbolic targets. The dispatcher resolves them to concrete hosts using a service directory built from discovery announcements. "Local execution" is just "resolved to my own host" — no special code path.

## Service Instance Keys

A `ServiceInstanceKey` identifies a deployable service in the mesh.

```rust
struct ServiceInstanceKey {
    category: ServiceCategory,
    implementation: String,
    scope: ServiceScope,
}

enum ServiceScope {
    Repo(RepoIdentity),    // API-backed: issues, change requests, cloud agents
    Host(HostName),        // process-managing: workspace, terminal, vcs
    Account(String),       // distinguishes two accounts for the same provider
}
```

`ServiceCategory` covers the current provider categories, with VCS absorbing CheckoutManager (checkout management is a sub-capability of VCS, not its own service).

### Service Instance Selectors

Steps target services through a `ServiceInstanceSelector`, not a fully-qualified key. A selector can omit fields to express "any implementation that matches":

```rust
struct ServiceInstanceSelector {
    category: ServiceCategory,
    implementation: Option<String>,
    scope: Option<ServiceScope>,
    // Future: tags like "fast", "cheap", "clever" for AI services
}
```

Fully-qualified selectors resolve to exactly one service instance. Partial selectors match against the directory — useful for "give me any issue tracker for this repo" without caring whether it's GitHub or Linear.

## StepExecutionContext Evolution

### Current

```rust
enum StepExecutionContext {
    Host(HostName),
    Environment(HostName, EnvironmentId),
}
```

Both variants carry a concrete `HostName`. The planner resolves eagerly; the step runner reads the answer off the step.

### Proposed

```rust
enum StepExecutionContext {
    /// Concrete host — post-resolution only. The planner never constructs this.
    Host(HostName),
    /// Run in an environment. Host resolved at dispatch from mesh state.
    Environment(EnvironmentId),
    /// Run on whichever host serves this service instance.
    Service(ServiceInstanceSelector),
}
```

**Key changes:**

- `Environment` drops its `HostName`. The environment-to-host mapping resolves at dispatch time. This enables future environments not tied to a daemon host (cloud VMs, Lambda, etc.).
- `Service(selector)` replaces the current pattern where `build_plan` manually picks `local_host` or `target_host` for each step.
- `Host(HostName)` survives for post-resolution use. The step runner resolves `Service` and `Environment` to `Host` before dispatching.
- The planner constructs `Service(...)` or `Environment(...)`. Never `Host(...)` directly.

### Step Categories

Current steps fall into three groups, each with a natural selector pattern:

| Group | Examples | Selector targets |
|-------|----------|-----------------|
| **Checkout-local** | CreateCheckout, RemoveCheckout, PrepareTerminal | `vcs:{impl}:{host}` — wherever the checkout lives |
| **Presentation** | AttachWorkspace, CreateTeleportWorkspace | `workspace:{impl}:{host}` — the workspace manager serving the requesting user |
| **Repo service** | OpenChangeRequest, LinkIssues, FetchCheckoutStatus, ArchiveSession, GenerateBranchName | `issues:github:{repo}`, `ai-utility:claude`, etc. |

"Presentation host" replaces the concept of "client-local." Attaching a workspace is requesting the workspace manager service — which host that runs on depends on where the user wants to interact, not necessarily where the client daemon runs.

## Service Directory

Each daemon discovers its own service capabilities during startup and refresh. It announces them to peers through the existing peer handshake/overlay protocol.

The **service directory** is a mesh-wide view: `ServiceInstanceKey → Vec<HostName>` (candidates). Each daemon maintains its copy from peer announcements.

### Resolution

1. Match the selector against directory entries. Exact matches first, then relaxed (partial selectors match any entry with compatible fields).
2. Multiple candidates: lexicographic tiebreak on host name. Deterministic, requires no coordination.
3. No candidates: error — service unavailable in the mesh.
4. Future: leader election replaces lexicographic tiebreak. Preference tables (cost, reliability, affinity) guide selection.

### Single-Daemon Case

Trivial: the only host in the directory is you. Every selector resolves to your host name.

## Eliminating HostName::local()

Three changes, separable from the larger service-host work:

1. **`main.rs`**: Derive host name from config or `gethostname` at the one entry point. Thread it explicitly everywhere. Stop using `HostName::local()` as fallback.
2. **`refresh.rs`**: Receive the daemon's host name as a parameter instead of calling `HostName::local()`.
3. **`HostName::local()` itself**: Remains as a bootstrap utility ("read the machine's hostname") but disappears from the execution pipeline. It is not a semantic concept — just a way to seed the daemon's name when config doesn't specify one.

## Local Execution Is Not Special

The current step runner has two code paths: local steps call `resolver.resolve()` directly; remote steps go through `RemoteStepExecutor::execute_batch()`. These should converge.

Local dispatch is just a batch sent to your own executor. The step runner resolves all symbolic contexts to `Host(name)`, batches consecutive steps for the same host, and dispatches each batch through the same interface — regardless of whether the target is local or remote.

## Relationship to #465

Issue #465 introduces Service as a first-class role distinct from Provider. The service-host concept here is complementary:

- #465 defines *what a service is* (query interface vs log publisher) and separates the issue list from the provider pipeline.
- This design defines *where a service runs* (service-host resolution) and how steps find it.

Together: providers publish to logs replicated across the mesh. Services expose query interfaces at a resolved host. Steps target services by selector; the mesh routes them.

## Relationship to Environments (#530)

Docker environments are the first case where an `EnvironmentId` resolves to a host at dispatch time rather than being baked in at plan time. The current `Environment(HostName, EnvironmentId)` variant works for #530 because the host is known (the daemon that created the container). But it bakes in an assumption that will break for cloud-provisioned environments.

This design prepares for that future by dropping the `HostName` from `Environment`. For #530, the resolution is straightforward: "the environment is on the host that created it." Later, resolution can query a cloud provider.

## What This Does Not Cover

- **Environment-in-environment** (a VM running a workspace manager): out of scope.
- **Cost-based or reliability-based routing**: future work. The selector/directory model supports it — selectors can grow tag filters, the directory can carry cost metadata — but the initial implementation uses lexicographic tiebreak.
- **Service lifecycle** (health checks, failover, deregistration): future work. The directory is eventually-consistent from peer announcements; a host going offline is detected through existing peer health mechanisms.
- **Teleport decomposition**: `TeleportSession` is an early proof-of-concept command that should be composed from other primitives. Separate cleanup task.
