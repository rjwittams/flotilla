# WorkflowTemplate Stage 2 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the `WorkflowTemplate` resource to `crates/flotilla-resources`, including schema types, semantic validation, CRUD/watch coverage, and a local validation example binary.

**Architecture:** Keep Stage 2 self-contained inside `crates/flotilla-resources`. Put the new resource schema and `validate()` logic in a dedicated `workflow_template` module, keep backend CRUD/watch behavior generic, and add WorkflowTemplate-specific tests/examples on top of the existing resource client infrastructure. Validation remains pure client-side logic: parse with serde first, then run semantic checks over typed data for DAG correctness and flotilla-owned interpolation references.

**Tech Stack:** Rust, serde, serde_yml, serde_json, tokio, futures, reqwest, chrono, hand-written CRD YAML.

---

**Spec:** `docs/superpowers/specs/2026-04-14-workflow-template-design.md`

**Execution notes:**
- Execute this in a dedicated worktree.
- Follow `@test-driven-development` for each task.
- Before final handoff, run `@verification-before-completion`.

## File Structure

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `crates/flotilla-resources/src/lib.rs` | Re-export `WorkflowTemplate` public API |
| Create | `crates/flotilla-resources/src/workflow_template.rs` | Resource marker, spec/status types, interpolation parser, `validate()`, `ValidationError` |
| Create | `crates/flotilla-resources/src/crds/workflow_template.crd.yaml` | Hand-written CRD schema for the new resource |
| Modify | `crates/flotilla-resources/examples/k8s_crud.rs` | Apply both CRDs and exercise WorkflowTemplate CRUD/watch |
| Create | `crates/flotilla-resources/examples/validate_workflow.rs` | Parse + validate a local WorkflowTemplate YAML file |
| Create | `crates/flotilla-resources/examples/review-and-fix.yaml` | Sample valid WorkflowTemplate document used by examples |
| Modify | `crates/flotilla-resources/tests/common/mod.rs` | Shared WorkflowTemplate test helpers alongside the existing convoy helpers |
| Create | `crates/flotilla-resources/tests/workflow_template_validation.rs` | Parse/validate test coverage for Stage 2 semantic rules |
| Create | `crates/flotilla-resources/tests/workflow_template_in_memory.rs` | WorkflowTemplate CRUD/watch tests against `InMemoryBackend` |
| Modify | `crates/flotilla-resources/tests/k8s_integration.rs` | Opt-in minikube CRUD/watch round-trip for WorkflowTemplate |
| Modify | `docs/superpowers/specs/2026-04-14-workflow-template-design.md` | Only if implementation reveals a real spec mismatch; otherwise leave unchanged |

## Chunk 1: Core Resource and Validation

### Task 1: Add the WorkflowTemplate module and public exports

**Files:**
- Create: `crates/flotilla-resources/src/workflow_template.rs`
- Modify: `crates/flotilla-resources/src/lib.rs`

- [ ] **Step 1: Write the compile-first public API sketch in the new module**

Add the exact public shapes from the spec:

```rust
pub struct WorkflowTemplate;

impl Resource for WorkflowTemplate {
    type Spec = WorkflowTemplateSpec;
    type Status = ();

    const API_PATHS: ApiPaths = ApiPaths {
        group: "flotilla.work",
        version: "v1",
        plural: "workflowtemplates",
        kind: "WorkflowTemplate",
    };
}
```

- [ ] **Step 2: Add the schema types without validation logic yet**

Include:
- `WorkflowTemplateSpec`
- `InputDefinition`
- `TaskDefinition`
- `ProcessDefinition`
- `ProcessSource`
- `Selector`
- `InterpolationLocation`
- `InterpolationField`
- `ValidationError`

Important serde details:
- `ProcessDefinition` uses `#[serde(flatten)]`
- `ProcessSource` uses `#[serde(untagged, deny_unknown_fields)]`
- `InputDefinition.description` is `Option<String>` with `#[serde(default)]`
- `TaskDefinition.depends_on` is `Vec<String>` with `#[serde(default)]`

- [ ] **Step 3: Re-export the new public API from `src/lib.rs`**

Export:
- `WorkflowTemplate`
- `WorkflowTemplateSpec`
- `InputDefinition`
- `TaskDefinition`
- `ProcessDefinition`
- `ProcessSource`
- `Selector`
- `ValidationError`
- `InterpolationLocation`
- `InterpolationField`
- `validate`

- [ ] **Step 4: Run a compile check before adding logic**

Run: `cargo build -p flotilla-resources --locked`

Expected:
- PASS
- New module compiles and public re-exports resolve

- [ ] **Step 5: Commit**

Run:
```bash
git add crates/flotilla-resources/src/lib.rs crates/flotilla-resources/src/workflow_template.rs
git commit -m "feat: add workflow template resource types"
```

### Task 2: Implement semantic validation and interpolation ownership rules

**Files:**
- Modify: `crates/flotilla-resources/src/workflow_template.rs`

- [ ] **Step 1: Write the failing validation tests inline or as temporary focused unit tests**

Cover the contract before implementation:
- duplicate task names
- duplicate input names
- duplicate role names within one task
- unknown dependency
- dependency cycle
- `{{inputs.name}}` success/failure
- `{{workflow.name}}` and `{{workflow.namespace}}` success
- malformed recognized tokens such as `{{inputs.}}` and `{{workflow.name.extra}}`
- passthrough of foreign tokens such as `{{.metadata.name}}`

Run: `cargo test -p flotilla-resources --locked workflow_template`

Expected:
- FAIL
- Missing `validate()` logic or wrong error set

- [ ] **Step 2: Implement `validate(spec) -> Result<(), Vec<ValidationError>>`**

Use one semantic pass that accumulates all errors:

```rust
pub fn validate(spec: &WorkflowTemplateSpec) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    // duplicates
    // dependency resolution
    // cycle detection
    // interpolation scan
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
```

Implementation requirements:
- collect every semantic error instead of returning early
- preserve task-local context for `DuplicateRoleInTask`
- preserve `task`, `role`, and `field` in interpolation errors

- [ ] **Step 3: Implement DAG validation with deterministic output**

Use:
- `BTreeSet` / ordered traversal for duplicate detection
- DFS with visitation state for cycle detection

Cycle output must match the spec style:
- `[a, b, c, a]`

Do not rely on task list order for execution semantics, but do keep error reporting deterministic for tests.

- [ ] **Step 4: Implement interpolation token scanning**

Rules to encode:
- only `inputs.*` and `workflow.*` are flotilla-owned
- foreign prefixes pass through unchanged
- recognized prefixes must be both well-formed and resolvable
- no internal whitespace in flotilla-owned tokens

Recommended helpers:

```rust
fn validate_template_text(
    text: &str,
    location: &InterpolationLocation,
    declared_inputs: &BTreeSet<String>,
    errors: &mut Vec<ValidationError>,
)

fn classify_token(token: &str) -> Option<OwnedInterpolation<'_>>
```

Where `OwnedInterpolation` distinguishes:
- `Input(&str)`
- `WorkflowName`
- `WorkflowNamespace`
- `Malformed`

- [ ] **Step 5: Make malformed recognized tokens fail even if they start with a valid scope**

Examples that must produce `MalformedInterpolation`:
- `{{inputs.}}`
- `{{inputs.branch }}`
- `{{workflow.name.extra}}`

Examples that must not produce errors:
- `{{.metadata.name}}`
- `{{ .Release.Name }}`
- `{{inptus.branch}}`

- [ ] **Step 6: Re-run the focused tests**

Run: `cargo test -p flotilla-resources --locked workflow_template`

Expected:
- PASS
- All new validation cases are green

- [ ] **Step 7: Commit**

Run:
```bash
git add crates/flotilla-resources/src/workflow_template.rs
git commit -m "feat: add workflow template validation"
```

### Task 3: Add dedicated parser and validation test coverage

**Files:**
- Create: `crates/flotilla-resources/tests/workflow_template_validation.rs`
- Modify: `crates/flotilla-resources/tests/common/mod.rs`
- Modify: `crates/flotilla-resources/examples/review-and-fix.yaml`

- [ ] **Step 1: Add shared WorkflowTemplate test helpers to `tests/common/mod.rs`**

Add helper constructors:
- `workflow_template_meta(name: &str) -> InputMeta`
- `valid_workflow_template_spec() -> WorkflowTemplateSpec`
- `valid_workflow_template_yaml() -> &'static str`

Keep these helpers separate from convoy helpers by naming, not by comments.

- [ ] **Step 2: Create `tests/workflow_template_validation.rs` with one test per error variant**

Minimum test set:
- `parse_rejects_process_without_selector_or_command`
- `parse_rejects_process_with_selector_and_command`
- `parse_rejects_prompt_on_tool_process`
- `validate_rejects_duplicate_task_names`
- `validate_rejects_duplicate_input_names`
- `validate_rejects_duplicate_role_names_within_task`
- `validate_rejects_unknown_dependencies`
- `validate_rejects_cycles`
- `validate_rejects_unknown_input_references`
- `validate_rejects_unknown_workflow_fields`
- `validate_rejects_malformed_owned_interpolations`
- `validate_allows_foreign_interpolations`

- [ ] **Step 3: Add round-trip coverage using the sample YAML**

Test shape:

```rust
let first: WorkflowTemplateSpec = serde_yml::from_str(sample_yaml)?;
let encoded = serde_yml::to_string(&first)?;
let second: WorkflowTemplateSpec = serde_yml::from_str(&encoded)?;
assert_eq!(second, first);
```

This requires deriving `PartialEq, Eq` on the schema and validation-support types where useful.

- [ ] **Step 4: Make `examples/review-and-fix.yaml` the single source of truth for the valid sample**

Use the same YAML in:
- the example binary
- parser round-trip tests

That prevents spec drift between docs and executable assets.

- [ ] **Step 5: Run the dedicated validation suite**

Run: `cargo test -p flotilla-resources --locked --test workflow_template_validation`

Expected:
- PASS
- Parser and semantic validator coverage are green

- [ ] **Step 6: Commit**

Run:
```bash
git add crates/flotilla-resources/tests/common/mod.rs \
        crates/flotilla-resources/tests/workflow_template_validation.rs \
        crates/flotilla-resources/examples/review-and-fix.yaml \
        crates/flotilla-resources/src/workflow_template.rs
git commit -m "test: add workflow template validation coverage"
```

## Chunk 2: Backends, Examples, and Cluster Coverage

### Task 4: Add the WorkflowTemplate CRD and in-memory CRUD/watch tests

**Files:**
- Create: `crates/flotilla-resources/src/crds/workflow_template.crd.yaml`
- Create: `crates/flotilla-resources/tests/workflow_template_in_memory.rs`
- Modify: `crates/flotilla-resources/tests/common/mod.rs`

- [ ] **Step 1: Write the failing in-memory round-trip test first**

Create:
- `create_get_list_roundtrip_for_workflow_templates`
- `update_requires_current_resource_version_for_workflow_templates`
- `delete_emits_deleted_event_for_workflow_templates`
- `watch_from_version_replays_update_then_delete_for_workflow_templates`

Do not add any `update_status` coverage for this resource.

Run: `cargo test -p flotilla-resources --locked --test workflow_template_in_memory`

Expected:
- FAIL
- Missing WorkflowTemplate helpers and/or CRD-independent test data

- [ ] **Step 2: Add the hand-written CRD asset**

Create `src/crds/workflow_template.crd.yaml` to match the spec exactly:
- namespaced resource
- group `flotilla.work`
- version `v1`
- no `subresources.status`
- `shortNames: [wft]`
- `oneOf` + `not` XOR enforcement for `selector` vs `command`

- [ ] **Step 3: Add WorkflowTemplate in-memory test helpers**

In `tests/common/mod.rs`, add:
- `pub struct WorkflowTemplateResource` only if you choose not to use the real exported `WorkflowTemplate`
- otherwise prefer using the real crate type directly and keep only metadata/spec fixtures in the test helper module

Preferred direction:
- use the real exported `WorkflowTemplate`
- keep helpers limited to fixture builders

- [ ] **Step 4: Make watch assertions reflect the status-less design**

The watch test must drive events with:
- `update(meta, resource_version, spec)`
- `delete(name)`

Expected event sequence after `list()` then `watch(FromVersion(...))`:
- `Modified` after spec update
- `Deleted` after delete

- [ ] **Step 5: Run the in-memory WorkflowTemplate suite**

Run: `cargo test -p flotilla-resources --locked --test workflow_template_in_memory`

Expected:
- PASS
- Resource version and watch semantics match the Stage 2 spec

- [ ] **Step 6: Commit**

Run:
```bash
git add crates/flotilla-resources/src/crds/workflow_template.crd.yaml \
        crates/flotilla-resources/tests/common/mod.rs \
        crates/flotilla-resources/tests/workflow_template_in_memory.rs
git commit -m "feat: add workflow template resource coverage"
```

### Task 5: Extend the example binaries for local validation and cluster CRUD

**Files:**
- Modify: `crates/flotilla-resources/examples/k8s_crud.rs`
- Create: `crates/flotilla-resources/examples/validate_workflow.rs`
- Create: `crates/flotilla-resources/examples/review-and-fix.yaml`

- [ ] **Step 1: Write the failing local validation example first**

Create `examples/validate_workflow.rs` with this flow:
- read file path from argv
- parse full YAML resource or `spec` payload into the chosen input shape
- run `validate()`
- print every semantic error on failure
- exit non-zero on parse or validation failure

Keep the interface exact:

```bash
cargo run -p flotilla-resources --example validate_workflow -- crates/flotilla-resources/examples/review-and-fix.yaml
```

- [ ] **Step 2: Add the valid sample YAML file**

Create `examples/review-and-fix.yaml` with:
- two declared inputs
- one root task
- one dependent task
- one agent process with prompt interpolation
- one tool process with command text

Keep it aligned with the spec sample so docs and executable examples stay in sync.

- [ ] **Step 3: Extend `examples/k8s_crud.rs`**

Change the example to:
- apply namespace
- apply both `convoy.crd.yaml` and `workflow_template.crd.yaml`
- parse + validate `examples/review-and-fix.yaml`
- create the WorkflowTemplate resource
- list it
- start a watch from the list resource version
- update the spec to trigger a `Modified` watch event
- delete it to trigger a `Deleted` watch event

Do not use `update_status`.

- [ ] **Step 4: Smoke-test both examples**

Run:
- `cargo build -p flotilla-resources --locked --example validate_workflow --example k8s_crud`
- `cargo run -p flotilla-resources --example validate_workflow -- crates/flotilla-resources/examples/review-and-fix.yaml`

Expected:
- build PASS
- validator prints success for the sample YAML

- [ ] **Step 5: Commit**

Run:
```bash
git add crates/flotilla-resources/examples/k8s_crud.rs \
        crates/flotilla-resources/examples/validate_workflow.rs \
        crates/flotilla-resources/examples/review-and-fix.yaml
git commit -m "feat: add workflow template examples"
```

### Task 6: Extend the opt-in minikube integration test and run full crate verification

**Files:**
- Modify: `crates/flotilla-resources/tests/k8s_integration.rs`
- Modify: `crates/flotilla-resources/src/lib.rs`
- Modify: `crates/flotilla-resources/src/workflow_template.rs`

- [ ] **Step 1: Add a WorkflowTemplate minikube test path to `tests/k8s_integration.rs`**

Preferred structure:
- keep the existing convoy test
- add a second ignored test for WorkflowTemplate, or extend the existing test to cover both resources in one function

Prefer a second test if that keeps failures more targeted:
- `workflow_template_crud_and_watch_roundtrip`

- [ ] **Step 2: Mirror the supported operations only**

Test flow:
- gate on `FLOTILLA_RUN_K8S_TESTS=1`
- build backend from kubeconfig
- ensure namespace
- ensure WorkflowTemplate CRD
- create resource
- list resources
- watch from listed collection `resourceVersion`
- update spec and assert `Modified`
- delete and assert `Deleted`

- [ ] **Step 3: Run the full crate test suite**

Run:
- `cargo test -p flotilla-resources --locked`
- If minikube is available: `FLOTILLA_RUN_K8S_TESTS=1 cargo test -p flotilla-resources --locked -- --ignored`

Expected:
- local crate tests PASS
- ignored cluster tests PASS when a cluster is reachable

- [ ] **Step 4: Run formatting and a final targeted build check**

Run:
- `cargo +nightly-2026-03-12 fmt --check`
- `cargo build -p flotilla-resources --locked`

Expected:
- PASS

- [ ] **Step 5: Commit**

Run:
```bash
git add crates/flotilla-resources/tests/k8s_integration.rs \
        crates/flotilla-resources/src/lib.rs \
        crates/flotilla-resources/src/workflow_template.rs \
        crates/flotilla-resources/examples/k8s_crud.rs \
        crates/flotilla-resources/examples/validate_workflow.rs \
        crates/flotilla-resources/examples/review-and-fix.yaml \
        crates/flotilla-resources/src/crds/workflow_template.crd.yaml \
        crates/flotilla-resources/tests/common/mod.rs \
        crates/flotilla-resources/tests/workflow_template_validation.rs \
        crates/flotilla-resources/tests/workflow_template_in_memory.rs
git commit -m "feat: add workflow template resource"
```

## Done Criteria

- `WorkflowTemplate` is exported from `crates/flotilla-resources`
- `validate()` returns all semantic errors in one pass
- owned interpolation handling matches the Stage 2 spec
- the CRD admits the intended YAML shape and rejects invalid process-source combinations
- in-memory CRUD/watch coverage exists for a status-less WorkflowTemplate
- the local validator example works without a cluster
- the minikube test path covers create/list/watch/update/delete for WorkflowTemplate
- `cargo test -p flotilla-resources --locked` passes

