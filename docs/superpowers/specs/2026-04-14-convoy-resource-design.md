# Convoy Resource and Controller — Design

## Context

Convoy is Stage 3 of the convoy implementation (see `docs/superpowers/specs/2026-04-13-convoy-brainstorm-prompts.md`). A Convoy is a named workflow instance: it references a `WorkflowTemplate`, carries inputs, and tracks per-task state as the DAG advances.

Stage 3 ships the resource, a reconciliation controller that advances tasks through the DAG, and a runnable example binary against minikube. It deliberately stops at the "task becomes Ready" boundary — actual provisioning (Stage 4) is the first consumer of that state. Presentation, TUI, CLI, and the `PersistentAgent` / policy work all live in later stages.

## Crate

Lives in the existing `crates/flotilla-resources` crate alongside `WorkflowTemplate`. New `convoy` module. Replaces the existing stub CRD at `src/crds/convoy.crd.yaml`.

## Scope

### In scope

- Rust `Convoy` type implementing `Resource`, with `ConvoySpec` / `ConvoyStatus` and the task state machine.
- Hand-written CRD YAML replacing the stub; namespaced, status subresource enabled, printer columns for `kubectl get cvy`.
- Pure `reconcile(convoy, spec, status, template, now) -> ReconcileOutcome` function.
- Example controller binary (`examples/convoy_controller.rs`) using list-then-watch + periodic resync.
- Table tests for `reconcile`, in-memory backend end-to-end test, HTTP backend integration test against minikube.
- Template snapshotting on first successful reconcile — the DAG is frozen into `convoy.status` at init.

### Out of scope (for this stage)

- Task provisioning, placement-policy resolution, container/environment creation.
- `PlacementPolicy` resource (Stage 4 or a sibling concern).
- `PersistentAgent` resource (future — houses Quartermaster, Yeoman, custom SDLC agents).
- Presentation / workspace integration (Stage 5).
- TUI / CLI surface (Stage 6+).
- Interactive launch UX (fetch template → auto-fill from context → approve).
- AttachableSet migration (Stage 7).
- Workflow composition (`includes`) and typed inputs — still deferred from Stage 2.

## Blue-sky Model (for orientation)

Stage 3's seams are designed around the following future split, captured here so the shape doesn't paint us in:

- **`WorkflowTemplate`** — shared, portable. *What to run, in what order.* Identical across deployment contexts.
- **`Convoy`** — workflow instance. *Which template, what inputs, which policy.*
- **`PlacementPolicy`** (future) — *where and how.* Named, with a default, possibly auto-discovered (today's `docker@host` style). Eventually delegates to or is implemented by a `PersistentAgent` (Quartermaster).
- **`PersistentAgent`** (future) — a single resource type with k8s-style labels/selectors. Conventional instances (Quartermaster, Yeoman, TestCoach, SecurityReviewer, …) are just labeled realizations. Agent runtime shape deliberately not committed: managed CLI (input-send), external CLI (shell-out), headless JSON/ACP, or internal LLM loop. All of them are presentable.
- **`PresentationManager`** (future) — scope-decoupled: full-flotilla / repo / convoy views.

Everything after `Convoy` is deferred. Stage 3's convoy carries an opaque `placement_policy: Option<String>` reference so Stage 4 can take over without a schema break.

## Resource Definition

### Rust

```rust
pub struct Convoy;
impl Resource for Convoy {
    type Spec = ConvoySpec;
    type Status = ConvoyStatus;
    const API_PATHS: ApiPaths = ApiPaths {
        group: "flotilla.work",
        version: "v1",
        plural: "convoys",
        kind: "Convoy",
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvoySpec {
    pub workflow_ref: String,                         // WorkflowTemplate name in same namespace
    #[serde(default)]
    pub inputs: BTreeMap<String, InputValue>,
    #[serde(default)]
    pub placement_policy: Option<String>,             // opaque; Stage 4 resolves
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputValue {
    String(String),
    // Future: Issue(IssueRef), IssueList(Vec<IssueRef>), Branch(BranchRef), ...
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConvoyStatus {
    pub phase: ConvoyPhase,
    #[serde(default)]
    pub tasks: BTreeMap<String, TaskState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_workflow_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_workflows: Option<BTreeMap<String, String>>, // ref → resourceVersion
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConvoyPhase {
    Pending,
    Active,
    Completed,
    Failed,
    Cancelled,
}

impl Default for ConvoyPhase {
    fn default() -> Self { ConvoyPhase::Pending }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub phase: TaskPhase,
    #[serde(default)]
    pub depends_on: Vec<String>,                      // snapshot, populated at init
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementStatus>,           // Stage 4 populates
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskPhase {
    Pending,
    Ready,
    Launching,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Placement metadata written by Stage 4's provisioning controller.
/// Shape is deferred; Stage 3 only reserves the field.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlacementStatus {
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}
```

### YAML

```yaml
apiVersion: flotilla.work/v1
kind: Convoy
metadata:
  name: fix-bug-123
  namespace: flotilla
spec:
  workflow_ref: review-and-fix
  inputs:
    feature: "Retry logic for the poller"
    branch: "fix-bug-123"
  placement_policy: laptop-docker
status:
  phase: Active
  observed_workflow_ref: review-and-fix
  observed_workflows:
    review-and-fix: "42"
  started_at: "2026-04-14T10:00:00Z"
  tasks:
    implement:
      phase: Running
      depends_on: []
      started_at: "2026-04-14T10:00:05Z"
    review:
      phase: Pending
      depends_on: [implement]
```

### Notes on shape

- **`observed_workflow_ref` + `observed_workflows`** are populated only after the controller successfully resolves the template and bootstraps task state. Callers watching "is this convoy actually tied to the template?" check status, not spec.
- **`observed_workflows` is a map**, not a single version field, so the future `includes` case (a workflow that pulls in other workflows) extends naturally — each snapshotted template gets an entry.
- **`TaskState.depends_on` is a snapshot** taken from the template at init. Reconcile reads this for DAG advancement; it never re-fetches the live template. Template edits after convoy start do not propagate.
- **`TaskState.placement`** is reserved for Stage 4. Stage 3 leaves it unset.
- **`ConvoyPhase::Cancelled`** is reserved for future user-initiated cancel; Stage 3 never produces it directly.
- **`InputValue` is untagged**, so today's YAML reads as plain scalars. When typed variants (`Issue`, `IssueList`, `Branch`) land, richer shapes slot in without a schema break.

## CRD YAML

Replaces `crates/flotilla-resources/src/crds/convoy.crd.yaml`. Namespaced, group `flotilla.work`, v1, status subresource enabled.

```yaml
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: convoys.flotilla.work
spec:
  group: flotilla.work
  scope: Namespaced
  names:
    plural: convoys
    singular: convoy
    kind: Convoy
    shortNames: [cvy]
  versions:
    - name: v1
      served: true
      storage: true
      subresources:
        status: {}
      additionalPrinterColumns:
        - name: Workflow
          type: string
          jsonPath: .spec.workflow_ref
        - name: Phase
          type: string
          jsonPath: .status.phase
        - name: Age
          type: date
          jsonPath: .metadata.creationTimestamp
      schema:
        openAPIV3Schema:
          type: object
          properties:
            spec:
              type: object
              required: [workflow_ref]
              properties:
                workflow_ref: { type: string, minLength: 1 }
                inputs:
                  type: object
                  additionalProperties: true
                placement_policy: { type: string, minLength: 1 }
            status:
              type: object
              properties:
                phase:
                  type: string
                  enum: [Pending, Active, Completed, Failed, Cancelled]
                observed_workflow_ref: { type: string }
                observed_workflows:
                  type: object
                  additionalProperties: { type: string }
                message: { type: string }
                started_at: { type: string, format: date-time }
                finished_at: { type: string, format: date-time }
                tasks:
                  type: object
                  additionalProperties:
                    type: object
                    required: [phase, depends_on]
                    properties:
                      phase:
                        type: string
                        enum: [Pending, Ready, Launching, Running, Completed, Failed, Cancelled]
                      depends_on:
                        type: array
                        items: { type: string }
                      started_at: { type: string, format: date-time }
                      finished_at: { type: string, format: date-time }
                      message: { type: string }
                      placement:
                        type: object
                        x-kubernetes-preserve-unknown-fields: true
```

- `subresources.status: {}` enables the `/status` subresource so status patches don't contend with spec edits.
- `inputs.additionalProperties: true` keeps the schema open for future typed `InputValue` variants. Rust holds the real shape.
- `placement` uses `x-kubernetes-preserve-unknown-fields: true` so Stage 4 can populate arbitrary metadata without a CRD bump.

## Reconciliation

### Pure function

```rust
pub fn reconcile(
    convoy: &ResourceObject<Convoy>,
    template: Option<&ResourceObject<WorkflowTemplate>>,
    now: DateTime<Utc>,
) -> ReconcileOutcome;

pub struct ReconcileOutcome {
    pub patch: Option<ConvoyStatus>,     // None = no change
    pub events: Vec<ConvoyEvent>,        // observability
}

pub enum ConvoyEvent {
    PhaseChanged { from: ConvoyPhase, to: ConvoyPhase },
    TaskPhaseChanged { task: String, from: TaskPhase, to: TaskPhase },
    TemplateNotFound { name: String },
    WorkflowRefChanged { from: String, to: String },
    MissingInput { name: String },
}
```

`ConvoyEvent` is purely for observability — the watch loop logs them via `tracing`. They are not persisted in the resource. A future addition may emit k8s `Event` resources from the same enum, but that's out of scope for Stage 3.

Pure, no I/O. The watch loop reads the convoy (and template on first resolve only), calls `reconcile`, applies the patch via `update_status`. Tests drive it directly.

### Reconcile steps (single pass)

1. **Template resolution and snapshot.**
   - If `status.observed_workflow_ref` is unset → this is init. Look up the template.
     - Not found → `phase = Failed`, message `"WorkflowTemplate '<ref>' not found"`, emit `TemplateNotFound`, return.
     - Found → initialize `status.tasks` with every template task at `TaskPhase::Pending`, snapshot each task's `depends_on`. Set `status.observed_workflow_ref = spec.workflow_ref`. Set `status.observed_workflows = {ref: resourceVersion}`.
   - If `status.observed_workflow_ref` is set and `spec.workflow_ref` differs → `phase = Failed`, message `"workflow_ref changed after convoy start; not supported"`, emit `WorkflowRefChanged`, return. (Re-running with a new template is a future concern.)
   - Otherwise, snapshot exists; do not refetch the template.

2. **Input validation (init only).**
   - Happens on the same reconcile as step 1 bootstrap — the template is in hand.
   - Every declared template input must appear in `spec.inputs`. Missing → `phase = Failed`, message `"missing input '<name>'"`, emit `MissingInput`.
   - Extra inputs (in spec but not declared) → informational event only, not a failure.
   - After init, the controller does not re-validate inputs. The template has been snapshotted and will not be re-fetched; the declared input set is fixed. User edits to `spec.inputs` after init are visible to Stage 4's task launcher but not re-checked by the convoy controller.

3. **Fail-fast.**
   - If any task is `TaskPhase::Failed`: set convoy `phase = Failed` and convoy `finished_at = now`. For every non-terminal sibling (not `Completed`/`Failed`/`Cancelled`), transition to `Cancelled` and set the task's `finished_at = now`. The failed task itself retains its `finished_at` as written by whoever marked it Failed. Return.

4. **DAG advancement.**
   - For each `Pending` task whose every `depends_on` entry maps to a task in `Completed`: transition to `Ready`, set task `started_at = now`.
   - No other transitions produced by the convoy controller. `Ready → Launching → Running → Completed` is driven by Stage 4 (provisioning) and external actors (explicit completion).

5. **Phase roll-up.**
   - All tasks `Completed` → `phase = Completed`, `finished_at = now`.
   - Any task past `Pending` but no terminal convoy state → `phase = Active`, `started_at = now` if unset.
   - Otherwise → `phase = Pending`.

### Watch loop (example binary)

```rust
async fn run(backend: &ResourceBackend, namespace: &str) -> Result<()> {
    let convoys = backend.using::<Convoy>(namespace);
    let templates = backend.using::<WorkflowTemplate>(namespace);

    // Catch-up: list then watch from the collection resourceVersion.
    let list = convoys.list().await?;
    for convoy in &list.items {
        reconcile_and_apply(&convoys, &templates, convoy).await?;
    }
    let mut events = convoys.watch(WatchStart::FromVersion(list.resource_version)).await?;

    let mut resync = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            Some(event) = events.next() => { reconcile_from_event(&convoys, &templates, event?).await?; }
            _ = resync.tick() => { resync_all(&convoys, &templates).await?; }
        }
    }
}
```

- **List-then-watch** (`WatchStart::FromVersion(collection_rv)`) ensures no gap if the controller starts after convoys already exist. `WatchStart::Now` would miss pre-existing convoys — wrong for a controller.
- **Templates are not watched.** Once snapshotted, they are read only at convoy init. Template edits do not affect running convoys. This removes a whole class of "what if the template changes under me?" failure modes.
- **Periodic resync** (~60s) guards against missed events / cache drift. Standard k8s controller pattern.

### Conflict handling

`update_status` returns `ResourceError::Conflict` if `resourceVersion` is stale. The controller re-fetches the convoy and retries up to a bounded number of times (3). If still conflicting, drop — the next watch event or resync tick will re-reconcile.

### Ownership contract

Single-writer per field; transitions on `tasks[*].phase` are partitioned across owners.

| Who | What they write |
|-----|-----------------|
| Convoy controller (Stage 3) | `status.phase`, `status.observed_workflow_ref`, `status.observed_workflows`, `status.message`, `status.started_at`, `status.finished_at`, `status.tasks[*].phase` (`Pending↔Ready`, fail-fast cancels, template-init), `status.tasks[*].depends_on` (at init), `tasks[*].started_at`/`finished_at` at those transitions |
| Provisioning controller (Stage 4, future) | `status.tasks[*].placement`, `status.tasks[*].phase` for `Ready→Launching→Running`, `tasks[*].started_at` at Launching |
| External actors (CLI, TUI, agent-side CLI) | `status.tasks[*].phase` for terminal transitions (`Completed`, `Failed`, `Cancelled`), `tasks[*].finished_at` at those |

Task completion is signalled by a direct `PATCH` against the `/status` subresource of the convoy. K8s supports multiple writers on `/status` (Deployment + HPA pattern); our disjoint-transition partition keeps ownership clear.

## Tests

### Table tests (pure `reconcile`)

- Fresh convoy, template found → bootstrap status: all tasks Pending with `depends_on` snapshot; `observed_workflow_ref` and `observed_workflows` set.
- Template not found → `phase = Failed` with clear message.
- Missing input → `phase = Failed`.
- Extra input (not declared) → informational event only; no failure.
- All deps satisfied on a Pending task → transition to Ready with `started_at`.
- Fan-out: three tasks with no deps → all three go Ready in one reconcile.
- Fan-in: A→C, B→C, A=Completed, B=Running → C stays Pending. B completes → C goes Ready.
- One task Failed → all non-terminal siblings → Cancelled, convoy `phase = Failed`.
- All tasks Completed → `phase = Completed`, `finished_at` set.
- `spec.workflow_ref` changed after init → `phase = Failed`.
- Template refetch does not happen after init (verify by passing `None` for template on second call after snapshot; reconcile proceeds from status alone).

### In-memory backend end-to-end

- Create `WorkflowTemplate` + `Convoy` in the in-memory backend.
- Run the controller loop against simulated status patches that advance tasks through Ready → Running → Completed.
- Assert sequence of convoy phase transitions and task-phase transitions.

### HTTP backend integration (minikube, gated)

- Apply both CRDs.
- Create a WorkflowTemplate with a two-task DAG (`implement` → `review`).
- Create a Convoy referencing it.
- Run the example controller binary in a background task.
- Patch `tasks.implement.phase = Completed` via `/status`; assert `review` moves to Ready.
- Patch `tasks.review.phase = Completed`; assert convoy `phase = Completed`.

## Example Binary

`crates/flotilla-resources/examples/convoy_controller.rs`:

- Accepts `--namespace` flag, defaults to `flotilla`.
- Bootstraps CRDs via `ensure_crd`.
- List-then-watch loop as above.
- Structured logging with `tracing` matching the codebase style.
- Runs against minikube by default via `HttpBackend::from_kubeconfig`.

## Deliverables

1. `Convoy` Rust type and `Resource` impl.
2. `ConvoySpec`, `ConvoyStatus`, `ConvoyPhase`, `TaskState`, `TaskPhase`, `InputValue`, `PlacementStatus`, `ReconcileOutcome`, `ConvoyEvent` types.
3. Pure `reconcile(convoy, template, now) -> ReconcileOutcome` function.
4. Convoy CRD YAML (replaces the stub).
5. Table tests for reconcile.
6. In-memory backend end-to-end test.
7. HTTP backend integration test against minikube.
8. `examples/convoy_controller.rs` — runnable controller binary.

No provisioning, no policy resolution, no presentation, no CLI surface beyond what the example needs.

## Design Decisions

### Tasks as convoy sub-status, not independent resources

One Convoy resource carrying a map of task states, versus a separate `ConvoyTask` resource per task. Per the design doc, sub-status is simpler for v1: no resource-per-task proliferation, no cross-resource watches. Promotion to independent resources is a well-understood migration (reachable later if we need per-task independent watches).

### Template snapshot at init; no template watching

Cascading template edits into running convoys produces too many failure modes — task renames, dep reshapes, removed tasks all break observed state. Snapshotting the DAG into `status.tasks` at init makes Stage 3 reconciliation depend only on the convoy's own status after bootstrap. Template edits affect only new convoys. Re-running a convoy with a newer template version is a future primitive (copy convoy, reset status, re-snapshot).

### `observed_workflows` as a map

Single-entry today (root → resourceVersion). When workflow composition (`includes`) lands, every snapshotted template — root plus includes — gets an entry. Naming the field as a map now avoids a schema change later.

### Direct status patch for task completion

K8s supports multiple writers on `/status` (Deployment + HPA touch the same Deployment's status). Partitioning transitions across writers — convoy controller for DAG advancement, external actors for terminal completions — gives clean ownership without an RPC-style back-channel through spec.

The alternative of a spec-side command queue (`spec.task_actions: [{task, action: Complete}]`) was rejected: controllers normally don't mutate their own spec, "mark complete" is an event not desired state, and there's no real gain over direct status patches.

### List-then-watch on the controller

`WatchStart::Now` would miss convoys that exist before the controller starts. The Stage 1 API was designed exactly for the list-then-watch pattern (collection resourceVersion → `WatchStart::FromVersion`) — use it for any controller that cares about pre-existing state.

### Placement as an opaque field

`TaskState.placement` is present in the schema so Stage 4 has a place to write, but its shape is not modelled in Stage 3 (`BTreeMap<String, serde_json::Value>` + `x-kubernetes-preserve-unknown-fields`). This lets Stage 4 iterate the placement model without CRD bumps. Stage 3 never writes to it.

### `placement_policy` on spec, not per-task

A single policy reference on the convoy, rather than per-task placement overrides inline in the convoy spec. Rationale:

- The policy (future `PlacementPolicy` resource) is the thing that decides per-task details, possibly delegating to a Quartermaster agent.
- Inline per-task overrides would duplicate what a policy controls, and make every consumer re-implement resolution logic.
- Launch-time override is expressed by writing a different policy into `spec.placement_policy` — it *is* the override. No separate override channel is needed.

### `ConvoyPhase::Cancelled` reserved, not produced in Stage 3

User-initiated convoy cancel is a real future feature but adds a control-plane verb (patch spec flag? delete convoy and let finalizer GC?) that deserves its own design round. Stage 3 reserves the phase so consumers can pattern-match today without later breaking them.

## Deferred Items (captured in `docs/superpowers/specs/2026-04-13-convoy-brainstorm-prompts.md`)

To add under "From Stage 3":

- **`PlacementPolicy` resource** — named, default, auto-discovered; delegates to or is implemented by a `PersistentAgent`. Stage 3 references by opaque string; Stage 4 reifies.
- **`PersistentAgent` resource** — one resource type with k8s-style labels/selectors. Quartermaster, Yeoman, TestCoach, etc. are conventionally-labeled instances. Agent runtime shape deliberately open (managed CLI, external CLI, headless JSON/ACP, internal LLM loop).
- **Presentation scope decoupling** — `PresentationManager` at full-flotilla / repo / convoy scopes.
- **Interactive launch UX** — CLI/TUI flow: fetch template → infer inputs from context (current branch, selected issues) → present for approval → create convoy.
- **Typed `InputValue` variants** — `Issue`, `IssueList`, `Branch`, `ChangeRequest`, etc. Requires matching `InputDefinition.kind` in WorkflowTemplate (Stage 2 revision).
- **Label-based workflow discovery** — e.g. `flotilla.work/accepts: issue` on WorkflowTemplate, for UI surfacing based on user selection context. May be subsumed by typed inputs.
- **Workflow composition (`includes`)** — sub-workflows, transitive snapshotting into `observed_workflows`.
- **Template versioning** — `spec.workflow_ref_revision` for convoys that want a specific template version.
- **Convoy re-run** — copy a convoy, reset status, re-snapshot against newer template.
- **Convoy cancellation** — user-initiated cancel producing `ConvoyPhase::Cancelled`.
- **Admission webhook / fast-feedback validation** — complements the client-side Convoy validator once shared-cluster workflows demand it.
