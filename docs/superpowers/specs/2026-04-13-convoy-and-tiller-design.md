# Convoy, Workflow, and Tiller Control Plane Design

## Context

Flotilla currently manages development workspaces through a provider-based model: providers discover and report items (checkouts, PRs, sessions, terminals), a correlation engine groups them into work items, and users interact via TUI/CLI. The `AttachableSet` is the closest thing to a "unit of work" — a group of terminals sharing a host, checkout, and environment.

This document captures the design direction for three interrelated concepts:

1. **Convoy** — the new primary unit of work, replacing AttachableSet
2. **Workflow** — a DAG of tasks that a convoy executes
3. **Tiller** — a minimal k8s-style control plane that manages these as resources

## Tiller: Micro Control Plane

### What It Is

Tiller is a minimal control plane implementing a subset of the Kubernetes API model: named objects, declared desired state, observed status, labels, reconciliation loops. It is not a k8s fork — it's a small server that speaks enough of the same language that k8s tooling and client libraries work where practical.

### Design Goals

- **Small.** Single binary, Rust. SQLite for state.
- **Workload-agnostic.** Manages resources, not containers specifically.
- **Subsettable.** Higher-level systems choose which resource types and controllers they need.
- **Discoverable.** Agents report what exists as first-class objects.
- **Growable.** Start on a single machine with UDS, extend to remote nodes, migrate to real k8s.

### Controller API Abstraction

The key architectural decision: controllers are written against a `ResourceClient` trait, not against a specific backing store. The same controller logic can run on:

```
ConvoyController, EnvironmentController, etc.
         |
    ResourceClient trait  (get/list/watch/create/update/delete)
         |
    +----+----------------+------------------+
    |                     |                  |
InProcessTiller      TillerHTTP         K8sREST
(SQLite, same       (UDS/TCP to        (raw REST calls,
 process)            tiller server)      real k8s cluster)
```

- **InProcessTiller**: Zero-dependency laptop case. Runs inside the flotilla daemon. SQLite-backed.
- **TillerHTTP**: Standalone tiller server, REST over UDS/SSH/TCP.
- **K8sREST**: Real k8s cluster via raw REST calls (reqwest + serde). Teams or power users.

We prefer raw REST over kube-rs for the k8s backend. The `ResourceClient` trait should reflect plain REST semantics (what Tiller would eventually expose), not kube-rs's `Api<T>` abstractions. For prototyping a single-node controller loop, a simple watch-and-react loop is clearer than kube-rs's reconciler framework. We already have reqwest and serde.

This means prototyping on real k8s is viable — raw REST is the first `ResourceClient` implementation, InProcessTiller comes later with scope defined by actual usage rather than speculation.

### Transport

- Local: REST over Unix domain socket. No TLS overhead.
- Remote (current plan): UDS forwarded over SSH.
- Remote (future): TCP with TLS.
- No gRPC. JSON over HTTP. Debuggable with curl.

### State

SQLite. resourceVersion derived from rowid. Atomic multi-resource updates via transactions.

### API Shape

Standard k8s URL structure:

```
/apis/{group}/{version}/{resource}
/apis/{group}/{version}/{resource}/{name}
/apis/{group}/{version}/{resource}/{name}/status
```

Watch via SSE or chunked transfer. resourceVersion maps to SQLite rowid.

### kubectl Compatibility

A basic subset: `get`, `apply`, `describe`, `delete`, `logs`. Requires discovery endpoints and enough metadata for kubectl to format output.

Open question: how much unnecessary discovery/negotiation kubectl does per invocation.

### What Tiller Skips

- Full k8s API compatibility (admission webhooks, version conversion, aggregation)
- RBAC (auth at transport layer)
- Networking model (no pod IPs, Services, Ingress)
- etcd (SQLite instead)
- Scheduler (direct node assignment initially)
- Built-in workload types (Deployment, ReplicaSet) — domain of the system built on top

## Convoy: The Primary Unit of Work

### The Fundamental Shift

**Before:** Branch -> Checkout -> (terminals appear) -> work happens
**After:** Convoy -> (creates branches, checkouts, environments, agents as needed) -> work happens

The convoy is the intent. Everything else is infrastructure it provisions. This is the spec/status split — the convoy spec says what to achieve, a controller reconciles reality toward it.

### Convoy Resource

```
Convoy {
    name: String,                    // explicit, meaningful name
    workflow: WorkflowRef,           // which template (or inline definition)
    status: ConvoyPhase,             // Pending -> Active -> Completed/Failed
    tasks: Vec<TaskStatus>,          // current state of each task in the DAG
}
```

A convoy is a named, user-created workflow instance. The name is explicit and meaningful (eventually could be AI-generated, similar to the existing branch name generation, or come from an initial planning conversation with an agent).

### Relationship to AttachableSet

Today's `AttachableSet` is a degenerate convoy — a convoy with a single task:

| AttachableSet field | Convoy equivalent |
|---------------------|-------------------|
| `id` (UUID) | Convoy name (meaningful string) |
| `host_affinity` | Task-level: which host the task runs on |
| `checkout` | Task-level: which checkout the task works against |
| `environment_id` | Task-level: which container/sandbox |
| `template_identity` | `workflow` reference |
| `members` (terminals) | Task's processes |

The critical difference: **host, checkout, and environment are Task-level properties, not Convoy-level.** A convoy can have tasks running on different hosts, different checkouts, different environments — sequentially or concurrently.

### Migration Path

Build Convoy as the first Tiller resource (option B). The current `AttachableSet` and `AttachableStoreApi` continue working during transition. Once convoys subsume all AttachableSet functionality, the old store is removed. No "two models" period for new work — convoys are born on the new model.

## Workflow: The DAG Shape

### WorkflowTemplate Resource

A reusable definition of the DAG shape, separate from any convoy instance:

```yaml
# .flotilla/workflows/review-and-fix.yaml
name: review-and-fix
tasks:
  - name: implement
    processes:
      - role: agent
        command: "{main_command}"
      - role: build
        command: "cargo watch -x check"

  - name: review
    depends_on: [implement]
    processes:
      - role: reviewer
        command: "claude --review"
      - role: tests
        command: "cargo test --watch"
```

Templates define **what runs and in what order**. They do not specify where — host, checkout, and environment are resolved at launch time when the convoy is instantiated.

### Task: The Placement Unit

A task is a node in the workflow DAG. All processes within a task share the same host, checkout, and environment. This is the scheduling/placement unit.

```
TaskDefinition {
    name: String,
    depends_on: Vec<String>,         // DAG edges
    processes: Vec<ProcessDefinition>,
}
```

### DAG Edges

For now, dependency edges mean **sequencing only**: task B starts after task A completes. Future possibilities (data flow, environment inheritance) are deferred.

### Task Lifecycle

```
Pending -> Ready -> Launching -> Running -> Completed
                                         -> Failed
                                         -> Cancelled
```

- **Pending**: dependencies not yet satisfied
- **Ready**: all dependencies completed, eligible to launch
- **Launching**: resources being provisioned (environment, checkout, terminals)
- **Running**: processes active, user can interact
- **Completed/Failed/Cancelled**: terminal states

For now, task completion is explicit — the user marks it done in the TUI, or process exit triggers a status change. Eventually agents will be able to mark their own task complete via a CLI command.

### Process: What Actually Runs

```
ProcessDefinition {
    role: String,                    // "coding-agent", "dev-server", "reviewer"
    command: String,                 // template-resolved command
}
```

A process is the logical thing ("coding agent on this checkout"). A terminal is how it's presented and interacted with. The process definition is resolved at launch time through the terminal pool and presentation manager.

Communication between processes within a task is not specified — for now, the user interacts with them via terminals. Future work may add explicit inter-process channels.

### Relationship to Current WorkspaceTemplate

Today's `WorkspaceTemplate` conflates two things:

- **Content** (roles + commands) = process definitions for a single task
- **Layout** (pane arrangement) = presentation manager configuration

In convoy terms: content moves into `WorkflowTemplate` task definitions, layout moves to `PresentationManager` config. A single-task workflow with a layout section is backwards-compatible with today's workspace templates.

## Presentation Flow

### How the Presentation Manager Surfaces Convoy Tasks

The presentation manager (currently `WorkspaceManager`) already receives fully-resolved `Vec<(role, command_string)>` pairs and a layout template. It doesn't know or care where the commands came from. The existing flow through `WorkspaceAttachRequest` → `resolve_template` → `PaneLayout` → create panes works unchanged for convoy tasks.

For a single convoy task becoming Ready:

```
Task becomes Ready
  → Controller resolves each ProcessDefinition through hop chain
  → Produces Vec<(role, command_string)>     ← same shape as today
  → Hands to PresentationManager with layout config
  → Panes created, user interacts
```

### Task Transitions

When task A completes and task B becomes Ready, the presentation needs to update. Options:

- **Reconfigure existing workspace** — add/replace panes for the new task's processes. Better UX (user stays in one context), but needs a new `update_workspace`/`add_panes` capability on the presentation manager trait.
- **Create new workspace** — simpler, but disorienting context switch.

Reconfiguration is the preferred direction.

### Convoy TUI Pane

A default convoy presentation could include a flotilla TUI pane focused on the convoy — showing task DAG progression alongside the terminal processes. This would be another process in the layout (e.g. `flotilla tui --convoy <name>`) running in its own pane, displaying task status, allowing the user to mark tasks complete, and navigating to process terminals.

## Provider Type Mapping

### Types That Become Convoy Sub-Resources (Created by Controllers)

These are things a convoy controller creates as it provisions tasks:

- **Environment** (from `EnvironmentProvider`)
- **Checkout** (from `CheckoutManager`)
- **Terminal sessions** (from `TerminalPool`)
- **Agent sessions** (from `CloudAgentService`)
- **Presentation/workspace** (from `WorkspaceManager`, renamed to `PresentationManager`)

### Types That Remain Read-Only Context

These don't need to be k8s resources to be useful — the convoy controller references them:

- PRs / change requests
- Issues
- Branches (remote)
- Commit info, working tree status

A convoy stage might reference external state ("wait for PR status = merged") by querying the existing provider, not by watching a k8s resource.

## Correlation

Convoy data feeds into the existing provider data / correlation engine as just another data source. The convoy controller doesn't need to know about correlation — it emits items with the right keys (Branch, CheckoutPath, etc.), and the correlation engine groups them with independently-discovered PRs, branches, and other items downstream. This means convoy integration with the current model is thin: convoys produce `ProviderData` items, correlation handles the rest. The exact long-term shape of correlation is a separate concern.

## Deferred Design Questions

These are real and important but orthogonal to the core convoy lifecycle:

### Agent-Planned Workflows
Agents should eventually be able to plan and modify workflow DAGs dynamically — replanning based on discovery, adding stages for progressive PRs, etc. For now, workflow templates are static.

### Multi-Branch Convoys
A workflow stage might cut a PR, get approval, then fork new branches for subsequent work. When the PR merges/squashes, downstream stages need to rebase/cherry-pick. This requires convoy-level branch management that doesn't exist yet.

### Agent Configuration
Process definitions will eventually need richer agent configuration: system prompt injection, permission policies (e.g. turn off permission checking if sandboxed), hooks/skills setup, config preparation. For now, `role + command` is sufficient.

### Inter-Convoy Coordination
Convoys are independent for now. Future work may need coordination between convoys (e.g. "this convoy depends on that convoy's output").

### Nesting / Sub-Convoys
A tempting model: "sub-convoy is-a stage" for scheduling purposes. Deferred because without concrete use cases driving it, we'd be speculating on the composition contract. The alternative — a convoy stage whose completion condition is "this other convoy completed" — gives inter-convoy coordination without nesting, and the pattern can be formalized later if it's common enough.

### Task as Independent Resource vs Convoy Sub-Status
In k8s terms: Argo makes each task its own resource (a Pod), giving independent watch/status. For flotilla's first cut, tasks-as-convoy-status is simpler — no need to watch tasks independently yet, avoids N-resources-per-convoy explosion. Can be promoted to independent resources later if needed.

## Naming

| Old | New |
|-----|-----|
| AttachableSet | Convoy (the workflow instance) |
| AttachableSet members | Task processes |
| WorkspaceTemplate | WorkflowTemplate (DAG) + PresentationConfig (layout) |
| WorkspaceManager | PresentationManager |

## Open Questions

- **kubectl overhead**: How chatty is kubectl per invocation? Is the discovery tax acceptable?
- **Discovered resource schema**: How to represent objects with observed state but no declared spec?
- **Node agent liveness**: Heartbeat model, timeout policy, disconnect behavior.
- **Dynamic vs static type registration**: CRD-like runtime registration vs compiled-in types?
- **Label/field selector query language**: How much of k8s selector syntax to support?
- **Prototyping strategy**: Start with real k8s (raw REST, standalone prototype) to validate the resource model, then build InProcessTiller once the API surface is known from actual usage.
