# Environment Provisioning End-to-End

**Issue:** #474 (Phase D of #442)
**Date:** 2026-03-25
**Depends on:** #471 (hop chain, complete), #472 (provider audit, complete), #473 (EnvironmentProvider + Docker, complete)

## Summary

Wire the environment provisioning infrastructure from Phase C into the step system, plan builder, checkout flow, hop chain, refresh pipeline, and sandbox socket lifecycle. Driven programmatically (tests construct Commands directly) — no CLI or TUI changes.

## Command Extension

`Command` gains `environment: Option<EnvironmentSpec>` next to `host`:

```rust
pub struct Command {
    pub host: Option<HostName>,
    pub environment: Option<EnvironmentSpec>,
    pub context_repo: Option<RepoSelector>,
    pub action: CommandAction,
}
```

`host` + `environment` together are the proto-`ProvisioningTarget`. `host` alone means bare host (today's behavior). `host` + `environment` means provision a container on that host. `#[serde(default, skip_serializing_if = "Option::is_none")]` for consistency with existing optional fields on `Command`.

## Data Model Corrections (from Phase C)

Phase C added `environment_id: Option<EnvironmentId>` to both `Checkout` and `CloudAgentSession`, and `environment_binding: Option<EnvironmentBinding>` to `RepoSnapshot`. Phase D corrects these:

- **Remove `environment_id` from `CloudAgentSession`** — cloud agent sessions (Claude, Codex, Cursor) run in their own sandboxes, not flotilla-managed environments. The field is meaningless on them.
- **Remove `environment_binding` from `RepoSnapshot`** — the repo-to-environment relationship is many-to-many (multiple environments per repo, multiple repos per environment). The correct model is `environment_id` on individual items (checkouts, terminals) plus host-level `EnvironmentInfo` in `HostSummary`. The `EnvironmentBinding` type can be removed from `flotilla-protocol`.
- **Keep `environment_id` on `Checkout`** — each checkout knows which environment it lives in.

## Step System

### New StepAction Variants

```rust
EnsureEnvironmentImage { spec: EnvironmentSpec },
CreateEnvironment { env_id: EnvironmentId, image: ImageId, opts: CreateOpts },
DiscoverEnvironmentProviders { env_id: EnvironmentId },
DestroyEnvironment { env_id: EnvironmentId },
```

Note: `EnsureRepoInEnvironment` is removed — see Codebase Access section. The `EnvironmentId` is passed to `CreateEnvironment` rather than generated inside `provider.create()`, so the step resolver can pre-allocate the socket and staging directory before calling the provider.

### StepHost Extension

```rust
pub enum StepHost {
    Local,
    Remote(HostName),
    Environment(EnvironmentId),
}
```

### Step Resolver

The `ExecutorStepResolver` gains:
- `environment_handles: HashMap<EnvironmentId, EnvironmentHandle>` — populated by `CreateEnvironment`, consumed by subsequent steps.
- `environment_registries: HashMap<EnvironmentId, Arc<ProviderRegistry>>` — populated by `DiscoverEnvironmentProviders`, used when `StepHost::Environment` routes actions through environment providers.
- `environment_sockets: Arc<Mutex<EnvironmentSocketRegistry>>` — passed in from the daemon server.

**Resolution of new actions:**

`EnsureEnvironmentImage { spec }` — looks up `EnvironmentProvider` from the host's registry, calls `ensure_image(spec)`. Returns `Produced(ImageId)`.

`CreateEnvironment { env_id, image, opts }` — the `env_id` is pre-allocated by the plan builder (UUID). The resolver creates the sandbox socket via `EnvironmentSocketRegistry::add(env_id, ...)` and populates `CreateOpts` with the socket path and reference repo mount. Calls `provider.create(env_id, image, opts)` (the provider API changes to accept the pre-allocated ID rather than generating one internally). Stores the `EnvironmentHandle`. Returns `Produced(EnvironmentId)`.

`DiscoverEnvironmentProviders { env_id }` — retrieves handle, calls `handle.env_vars()` to get raw `HashMap<String, String>`. Runs the host-level and repo-level detectors through the environment runner to build an `EnvironmentBag` from the container's environment (same detection pipeline as host discovery, routed through the runner). Then runs `FactoryRegistry::probe()` with the environment's `EnvironmentBag` and runner. Stores the resulting per-environment `ProviderRegistry`. Returns `Completed`.

`DestroyEnvironment { env_id }` — calls `handle.destroy()`, removes sandbox socket via `EnvironmentSocketRegistry::remove()`. Returns `Completed`.

**Routing for `StepHost::Environment(env_id)`:** The resolver looks up the environment's `ProviderRegistry` and routes the step's action through those providers instead of the host's. Existing step actions (checkout, terminal prep, workspace creation) work unchanged — they just run against different providers.

## Plan Builder

`build_plan()` in `executor.rs` checks `cmd.environment`. When present and the command involves checkout/workspace creation, it prepends environment lifecycle steps:

```
1. EnsureEnvironmentImage { spec }             on Local
2. CreateEnvironment { env_id, image, opts }   on Local
3. DiscoverEnvironmentProviders { env_id }     on Local
4. CreateCheckout { branch, ... }              on Environment(env_id)
5. PrepareTerminalForCheckout { ... }          on Environment(env_id)
6. CreateWorkspaceFromPreparedTerminal { ... } on Environment(env_id)
7. ResolveAttachCommand { ... }                → HopPlan with EnterEnvironment
```

**Why `Local`, not `Remote(host)`:** Today the whole `Command` is forwarded to the target host's daemon via peer routing (based on `Command.host`). That daemon plans and executes all steps locally. Step-level remote dispatch (#464) is a separate concern — Phase D does not require it. All steps are `Local` from the executing daemon's perspective.

Steps 1-3 manage the environment on the host. Steps 4-6 run inside the environment (routed through the environment's providers via `StepHost::Environment`). Step 7 produces a hop plan with `EnterEnvironment` for correct attach resolution.

`CreateOpts` is populated by the plan builder:
- `daemon_socket_path` — pre-created via `EnvironmentSocketRegistry::add(env_id, ...)`
- `reference_repo` — resolved from the host repo's git common dir (`git rev-parse --git-common-dir`), mounted read-only at `/ref/repo` inside the container
- `tokens` — passed through from `Command` context (Phase D: programmatic, Phase E: from config)

The reference repo is mounted directly as a single `-v` bind mount at container creation time. This is a Docker-specific optimisation for fast `git clone --reference`. For VMs or cloud instances (future), the checkout strategy would clone from the remote without a reference. Multi-repo environments would require multiple mounts at creation time or a different strategy; deferred as an open question.

## CloneCheckoutManager

New `CheckoutManager` implementation for environments. Discovered inside the container by its factory when the `EnvironmentBag` indicates a container context (presence of `FLOTILLA_ENVIRONMENT_ID` env var and `/ref/repo` reference mount).

```rust
struct CloneCheckoutManager {
    runner: Arc<dyn CommandRunner>,
    reference_dir: ExecutionEnvironmentPath,  // /ref/repo
}
```

`create_checkout(branch)` → `git clone --reference /ref/repo <remote_url> /workspace/<branch>`. The remote URL is read from the reference: `git --git-dir /ref/repo remote get-url origin`. For fresh branches, clones with `--no-checkout` then `git checkout -b <branch>` from the default branch.

Uses the same `CheckoutManager` trait as the worktree implementation. The plan builder and step resolver don't know about the difference — they call `create_checkout()` and the discovered provider handles the rest.

**Failure/rollback:** If a mid-plan step fails after the environment is created (e.g., checkout fails), the container is left running. Phase D does not add automatic rollback — `run_step_plan` stops on first error. Cleanup is manual (`docker rm -f`) or via a future `DestroyEnvironment` command. Automatic compensating actions are deferred.

### Factory

`CloneCheckoutManagerFactory` probes for:
- `FLOTILLA_ENVIRONMENT_ID` in `EnvironmentBag` (we're inside a container)
- `/ref/repo` exists and is a valid git directory (reference mount is available)

If both conditions are met, it returns a `CloneCheckoutManager` pointed at `/ref/repo`. Priority should be higher than the worktree factory inside environments (worktree creation doesn't make sense inside a disposable container).

## Attachable Environment Awareness

`AttachableSet` gains `environment_id: Option<EnvironmentId>`. When terminals are allocated inside an environment, the set is tagged with the environment ID. This field is the data path that the hop chain builder reads to know when to insert `EnterEnvironment` hops.

`build_for_prepared_command()` also needs environment context — it gains an `environment_id: Option<EnvironmentId>` parameter, passed by the workspace orchestrator when building commands for environment-hosted panes.

## Hop Chain Wiring

### HopPlanBuilder

`build_for_attachable()` and `build_for_prepared_command()` gain environment awareness. When the target attachable set carries `environment_id` (read from `AttachableStore`), or when the caller passes an explicit `environment_id`, the builder inserts `Hop::EnterEnvironment` between `RemoteToHost` and the terminal/command hop:

```
RemoteToHost(feta) → EnterEnvironment(env_id, "docker") → AttachTerminal(sess)
```

### Workspace Orchestrator

`resolve_prepared_commands_via_hop_chain()` in `executor/workspace.rs` currently uses `NoopEnvironmentHopResolver`. When creating a workspace inside an environment, it constructs `DockerEnvironmentHopResolver` with the container name mapping and passes it to the `HopResolver`. The mapping comes from the `EnvironmentHandle` in resolver state — `DockerProvisionedEnvironment` knows its container name internally, exposed via a method that the resolver calls to build the `EnvironmentId → container_name` map.

## Refresh and Host Summary

`refresh_providers()` in `refresh.rs` gains a call to `EnvironmentProvider::list()` alongside existing provider refreshes. Results populate `ProviderData` with environment info.

`build_local_host_summary()` reads environment provider results and populates `HostSummary.environments` with `EnvironmentInfo` entries. Remote daemons see environment availability via the host summary exchange.

## Sandbox Socket Lifecycle

`CreateEnvironment` step resolver (env_id is pre-allocated):
1. Calls `EnvironmentSocketRegistry::add(env_id, state_dir, spawn_fn)` → gets socket path
2. Populates `CreateOpts` with socket path and reference repo mount
3. Calls `EnvironmentProvider::create(env_id, image, opts)` — container starts with socket and repo mounted

`DestroyEnvironment` step resolver:
1. Calls `handle.destroy()` — container removed
2. Calls `EnvironmentSocketRegistry::remove(env_id)` — socket cleaned up

The `spawn_fn` closure creates an accept loop calling `handle_client` with `environment_context: Some(env_id)`.

`ExecutorStepResolver` gains `environment_sockets: Arc<Mutex<EnvironmentSocketRegistry>>`, passed from the daemon server.

## Testing

### Unit tests

- Plan builder produces correct step sequence when `command.environment` is `Some`
- Step resolver handles each new `StepAction` variant with mock providers
- `CloneCheckoutManager` calls correct git commands through mock runner
- `CloneCheckoutManagerFactory` probes correctly for container indicators

### In-process daemon test

Construct `Command { host: Some(feta), environment: Some(spec), action: Checkout { ... } }`, execute through `InProcessDaemon`. Verify full step sequence: ensure image → create environment → discover → checkout → terminals → workspace. Verify attach command resolves with `EnterEnvironment` hop. All mock-backed via replay fixtures.

### Real Docker (optional, not CI)

Same flow with `REPLAY=passthrough` against real Docker using the `flotilla-dev-env` image. Validates the entire chain against a real container.

## Dependencies

- **#464 (step-level remote routing)** — not required for Phase D. Today the whole command is forwarded to the target host's daemon, which plans and executes locally. Phase D uses `StepHost::Local` for all host-side steps. #464 enables future plans where individual steps target different hosts, but the single-host environment case works without it.

## Open Questions

- **Multi-repo environments:** A single bind mount per container limits each environment to one reference repo. Multiple repos would need multiple `-v` mounts at creation time (requires advance knowledge) or a different strategy (full clone from remote). Deferred.
- **Environment reuse:** Phase D creates a new environment per checkout command. Sharing an existing environment (e.g., "use the container that's already running for this branch") requires environment lookup and lifecycle management beyond create/destroy.
- **EnvironmentProvider API change:** `create()` currently generates the `EnvironmentId` internally. Phase D needs pre-allocated IDs (for socket setup before container creation). The `DockerEnvironment::create()` signature must change to accept an `EnvironmentId` parameter.

## Not in Scope (Phase E)

- CLI noun/verb changes (environment noun, provisioning target routing)
- TUI provisioning target UI (replacing target host)
- `.flotilla/environment.yaml` parsing
- Token config resolution (tokens passed programmatically)
- `ProvisioningTarget` enum (proto-form is `host` + `environment` on `Command`)
- Step-level remote routing (#464)
- Multi-repo environment support
