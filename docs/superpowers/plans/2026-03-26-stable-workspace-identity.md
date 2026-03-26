# Stable Workspace Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Switch cmux, zellij, and tmux workspace providers to stable identifiers, remove `Workspace.directories`, rewrite `select_existing_workspace` via binding lookups, and add scoped stale-binding pruning.

**Architecture:** The binding system already stores workspace identity as opaque strings. We swap unstable refs (positional/name-based) for stable IDs (UUIDs, tab_ids, window_ids), enforce a 1:1 workspace→set binding invariant, add a reverse lookup, and prune dead bindings during refresh using a provider-declared scope prefix.

**Tech Stack:** Rust, async-trait, serde_json (cmux/zellij JSON parsing), tokio

**Spec:** `docs/superpowers/specs/2026-03-26-cmux-stable-uuid-bindings-design.md`

---

### File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/flotilla-core/src/attachable/store.rs` | Modify | 1:1 invariant in `replace_binding`, add `lookup_workspace_ref_for_set` |
| `crates/flotilla-core/src/attachable/store/tests.rs` | Modify | Contract tests for new store behavior |
| `crates/flotilla-protocol/src/provider_data.rs` | Modify | Remove `directories` from `Workspace` |
| `crates/flotilla-core/src/providers/workspace/mod.rs` | Modify | Add `binding_scope_prefix()` to trait |
| `crates/flotilla-core/src/providers/workspace/cmux.rs` | Modify | UUID-based identity |
| `crates/flotilla-core/src/providers/workspace/zellij.rs` | Modify | session:tab_id identity, remove state files |
| `crates/flotilla-core/src/providers/workspace/tmux.rs` | Modify | start_time:session:@window_id identity, remove state files |
| `crates/flotilla-core/src/executor/workspace.rs` | Modify | Rewrite `select_existing_workspace` |
| `crates/flotilla-core/src/refresh.rs` | Modify | Stale binding pruning |

---

### Task 1: Enforce 1:1 binding invariant in `replace_binding`

**Files:**
- Modify: `crates/flotilla-core/src/attachable/store.rs:234-248`
- Test: `crates/flotilla-core/src/attachable/store/tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/flotilla-core/src/attachable/store/tests.rs`:

```rust
fn contract_replace_binding_enforces_one_to_one_for_sets(store: &mut impl AttachableStoreApi) {
    // First binding: workspace:old -> set-1
    store.replace_binding(ProviderBinding {
        provider_category: "workspace_manager".into(),
        provider_name: "cmux".into(),
        object_kind: BindingObjectKind::AttachableSet,
        object_id: "set-1".into(),
        external_ref: "workspace:old".into(),
    });

    // Second binding: workspace:new -> set-1 (same set, different ref)
    store.replace_binding(ProviderBinding {
        provider_category: "workspace_manager".into(),
        provider_name: "cmux".into(),
        object_kind: BindingObjectKind::AttachableSet,
        object_id: "set-1".into(),
        external_ref: "workspace:new".into(),
    });

    // The old binding should be gone — only workspace:new -> set-1 should exist
    assert!(store.lookup_binding("workspace_manager", "cmux", BindingObjectKind::AttachableSet, "workspace:old").is_none());
    assert_eq!(
        store.lookup_binding("workspace_manager", "cmux", BindingObjectKind::AttachableSet, "workspace:new"),
        Some("set-1")
    );

    // Bindings for Attachable kind should NOT be affected by 1:1 invariant
    store.replace_binding(ProviderBinding {
        provider_category: "terminal_pool".into(),
        provider_name: "shpool".into(),
        object_kind: BindingObjectKind::Attachable,
        object_id: "att-1".into(),
        external_ref: "flotilla/main/main/0".into(),
    });
    store.replace_binding(ProviderBinding {
        provider_category: "terminal_pool".into(),
        provider_name: "shpool".into(),
        object_kind: BindingObjectKind::Attachable,
        object_id: "att-1".into(),
        external_ref: "flotilla/main/agents/0".into(),
    });
    // Both should survive — 1:1 only applies to AttachableSet
    assert!(store.lookup_binding("terminal_pool", "shpool", BindingObjectKind::Attachable, "flotilla/main/main/0").is_some());
    assert!(store.lookup_binding("terminal_pool", "shpool", BindingObjectKind::Attachable, "flotilla/main/agents/0").is_some());
}

#[test]
fn file_backed_contract_replace_binding_enforces_one_to_one_for_sets() {
    contract_replace_binding_enforces_one_to_one_for_sets(&mut AttachableStore::with_base(&temp_base(
        &tempfile::tempdir().expect("tempdir"),
    )));
}

#[test]
fn in_memory_contract_replace_binding_enforces_one_to_one_for_sets() {
    contract_replace_binding_enforces_one_to_one_for_sets(&mut InMemoryAttachableStore::new());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core replace_binding_enforces_one_to_one --locked`
Expected: FAIL — the old binding `workspace:old` still exists because `replace_binding` only removes by `external_ref`, not by `object_id`.

- [ ] **Step 3: Implement the 1:1 invariant**

In `crates/flotilla-core/src/attachable/store.rs`, replace the `replace_binding` method on `AttachableStoreState` (lines 234-248) with:

```rust
    fn replace_binding(&mut self, binding: ProviderBinding) -> bool {
        if self.registry.bindings.iter().any(|existing| existing == &binding) {
            return false;
        }
        let key = Self::binding_key(&binding.provider_category, &binding.provider_name, &binding.object_kind, &binding.external_ref);
        self.binding_index.insert(key, binding.object_id.clone());

        // Collect stale index keys before mutating the bindings vec.
        // For AttachableSet bindings, enforce 1:1: remove any existing binding
        // for the same (category, name, kind, object_id) — ensures one workspace per set.
        let stale_keys: Vec<(String, String, BindingObjectKind, String)> = self
            .registry
            .bindings
            .iter()
            .filter(|existing| {
                existing.provider_category == binding.provider_category
                    && existing.provider_name == binding.provider_name
                    && existing.object_kind == binding.object_kind
                    && (existing.external_ref == binding.external_ref
                        || (binding.object_kind == BindingObjectKind::AttachableSet && existing.object_id == binding.object_id))
            })
            .map(|existing| {
                (
                    existing.provider_category.clone(),
                    existing.provider_name.clone(),
                    existing.object_kind.clone(),
                    existing.external_ref.clone(),
                )
            })
            .collect();

        for (cat, name, kind, ext_ref) in &stale_keys {
            let old_key = Self::binding_key(cat, name, kind, ext_ref);
            self.binding_index.remove(&old_key);
        }

        self.registry.bindings.retain(|existing| {
            !(existing.provider_category == binding.provider_category
                && existing.provider_name == binding.provider_name
                && existing.object_kind == binding.object_kind
                && (existing.external_ref == binding.external_ref
                    || (binding.object_kind == BindingObjectKind::AttachableSet && existing.object_id == binding.object_id)))
        });

        // Re-insert the new key (may have been removed by stale key cleanup above)
        let key = Self::binding_key(&binding.provider_category, &binding.provider_name, &binding.object_kind, &binding.external_ref);
        self.binding_index.insert(key, binding.object_id.clone());
        self.registry.bindings.push(binding);
        true
    }
```

- [ ] **Step 4: Run tests to verify**

Run: `cargo test -p flotilla-core --locked -- replace_binding`
Expected: All `replace_binding` tests pass, including the new 1:1 test and the existing `contract_replacing_binding_is_deterministic`.

- [ ] **Step 5: Commit**

```
fix: enforce 1:1 workspace binding invariant in replace_binding
```

---

### Task 2: Add `lookup_workspace_ref_for_set`

**Files:**
- Modify: `crates/flotilla-core/src/attachable/store.rs:35-87` (trait), plus all impls
- Test: `crates/flotilla-core/src/attachable/store/tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/flotilla-core/src/attachable/store/tests.rs`:

```rust
fn contract_lookup_workspace_ref_for_set(store: &mut impl AttachableStoreApi) {
    // No bindings — should return None
    let set_id = AttachableSetId::new("set-1");
    assert!(store.lookup_workspace_ref_for_set("workspace_manager", "cmux", &set_id).is_none());

    // Add a workspace binding
    store.replace_binding(ProviderBinding {
        provider_category: "workspace_manager".into(),
        provider_name: "cmux".into(),
        object_kind: BindingObjectKind::AttachableSet,
        object_id: "set-1".into(),
        external_ref: "ABC-UUID-123".into(),
    });

    // Should find it
    assert_eq!(
        store.lookup_workspace_ref_for_set("workspace_manager", "cmux", &set_id),
        Some("ABC-UUID-123".to_string())
    );

    // Different provider name — should not find it
    assert!(store.lookup_workspace_ref_for_set("workspace_manager", "tmux", &set_id).is_none());

    // Attachable bindings for the same object_id should not match
    store.replace_binding(ProviderBinding {
        provider_category: "terminal_pool".into(),
        provider_name: "shpool".into(),
        object_kind: BindingObjectKind::Attachable,
        object_id: "set-1".into(),
        external_ref: "flotilla/main/main/0".into(),
    });
    // Still only returns the workspace_manager binding
    assert_eq!(
        store.lookup_workspace_ref_for_set("workspace_manager", "cmux", &set_id),
        Some("ABC-UUID-123".to_string())
    );
}

#[test]
fn file_backed_contract_lookup_workspace_ref_for_set() {
    contract_lookup_workspace_ref_for_set(&mut AttachableStore::with_base(&temp_base(
        &tempfile::tempdir().expect("tempdir"),
    )));
}

#[test]
fn in_memory_contract_lookup_workspace_ref_for_set() {
    contract_lookup_workspace_ref_for_set(&mut InMemoryAttachableStore::new());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core lookup_workspace_ref_for_set --locked`
Expected: FAIL — method doesn't exist yet.

- [ ] **Step 3: Add method to trait and implement**

Add to the `AttachableStoreApi` trait in `crates/flotilla-core/src/attachable/store.rs` (after `lookup_binding`):

```rust
    fn lookup_workspace_ref_for_set(
        &self,
        provider_category: &str,
        provider_name: &str,
        set_id: &AttachableSetId,
    ) -> Option<String>;
```

Add implementation in `AttachableStoreState`:

```rust
    fn lookup_workspace_ref_for_set(
        &self,
        provider_category: &str,
        provider_name: &str,
        set_id: &AttachableSetId,
    ) -> Option<String> {
        self.registry
            .bindings
            .iter()
            .find(|b| {
                b.provider_category == provider_category
                    && b.provider_name == provider_name
                    && b.object_kind == BindingObjectKind::AttachableSet
                    && b.object_id == set_id.to_string()
            })
            .map(|b| b.external_ref.clone())
    }
```

Add delegation in `AttachableStore`:

```rust
    pub fn lookup_workspace_ref_for_set(
        &self,
        provider_category: &str,
        provider_name: &str,
        set_id: &AttachableSetId,
    ) -> Option<String> {
        self.state.lookup_workspace_ref_for_set(provider_category, provider_name, set_id)
    }
```

Add delegation in the `impl AttachableStoreApi for AttachableStore` block and the `impl AttachableStoreApi for InMemoryAttachableStore` block — both delegate to `self.state.lookup_workspace_ref_for_set(...)`.

- [ ] **Step 4: Run tests to verify**

Run: `cargo test -p flotilla-core lookup_workspace_ref_for_set --locked`
Expected: PASS

- [ ] **Step 5: Commit**

```
feat: add reverse binding lookup for workspace→set mapping
```

---

### Task 3: Remove `directories` from `Workspace` and fix compilation

**Files:**
- Modify: `crates/flotilla-protocol/src/provider_data.rs:320-327`
- Modify: `crates/flotilla-core/src/providers/workspace/cmux.rs` (constructors)
- Modify: `crates/flotilla-core/src/providers/workspace/zellij.rs` (constructors)
- Modify: `crates/flotilla-core/src/providers/workspace/tmux.rs` (constructors)
- Modify: `crates/flotilla-core/src/executor/workspace.rs:145-166` (select_existing_workspace)
- Modify: `crates/flotilla-core/src/data.rs` (if any references)
- Modify: various test files

- [ ] **Step 1: Remove the field from the struct**

In `crates/flotilla-protocol/src/provider_data.rs`, change the `Workspace` struct from:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub directories: Vec<PathBuf>,
    pub correlation_keys: Vec<CorrelationKey>,
    #[serde(default)]
    pub attachable_set_id: Option<AttachableSetId>,
}
```

to:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub correlation_keys: Vec<CorrelationKey>,
    #[serde(default)]
    pub attachable_set_id: Option<AttachableSetId>,
}
```

- [ ] **Step 2: Fix all compilation errors**

Run `cargo check --workspace --locked 2>&1` and fix each error. The errors will be in:

1. **cmux.rs** `parse_workspaces` and `create_workspace` — remove `directories` from `Workspace` constructors
2. **zellij.rs** `list_workspaces` and `create_workspace` — remove `directories` from constructors
3. **tmux.rs** `list_workspaces` and `create_workspace` — remove `directories` from constructors
4. **executor/workspace.rs** `select_existing_workspace` — temporarily stub to `false` (rewritten in Task 7):

```rust
    async fn select_existing_workspace(&self, _ws_mgr: &dyn WorkspaceManager, _checkout_path: &Path) -> bool {
        // TODO(Task 7): rewrite to use binding-based lookup
        false
    }
```

5. **Test files** — remove `directories` from any `Workspace` constructors in `data/tests.rs`, `cmux.rs` tests, etc. Remove assertions about `ws.directories`.

- [ ] **Step 3: Run full check and tests**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Run: `cargo test --workspace --locked`
Expected: All pass (select_existing_workspace is stubbed, not breaking anything — it just always creates new workspaces).

- [ ] **Step 4: Commit**

```
refactor: remove directories from Workspace struct

Stubbed select_existing_workspace — will be rewritten to use
binding-based lookup in a follow-up.
```

---

### Task 4: Add `binding_scope_prefix` to `WorkspaceManager` trait

**Files:**
- Modify: `crates/flotilla-core/src/providers/workspace/mod.rs:14-19`
- Modify: `crates/flotilla-core/src/providers/workspace/cmux.rs`
- Modify: `crates/flotilla-core/src/providers/workspace/zellij.rs`
- Modify: `crates/flotilla-core/src/providers/workspace/tmux.rs`

- [ ] **Step 1: Add the method to the trait**

In `crates/flotilla-core/src/providers/workspace/mod.rs`, add to the `WorkspaceManager` trait:

```rust
    /// Returns a prefix that all ws_refs from this provider instance will start with.
    /// Only bindings matching this prefix should be pruned based on the live workspace list.
    /// Returns empty string if list_workspaces() is exhaustive.
    fn binding_scope_prefix(&self) -> String;
```

- [ ] **Step 2: Implement for all providers**

**cmux.rs** — add to `impl WorkspaceManager for CmuxWorkspaceManager`:
```rust
    fn binding_scope_prefix(&self) -> String {
        String::new()
    }
```

**zellij.rs** — add to `impl WorkspaceManager for ZellijWorkspaceManager`:
```rust
    fn binding_scope_prefix(&self) -> String {
        match self.session_name() {
            Ok(session) => format!("{session}:"),
            Err(_) => String::new(),
        }
    }
```

**tmux.rs** — the tmux provider doesn't yet have `start_time` or a session name cached. For now, return empty (will be populated when the provider is updated in Task 6). Add to `impl WorkspaceManager for TmuxWorkspaceManager`:
```rust
    fn binding_scope_prefix(&self) -> String {
        String::new()
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --workspace --locked`
Expected: PASS

- [ ] **Step 4: Commit**

```
feat: add binding_scope_prefix to WorkspaceManager trait
```

---

### Task 5: cmux provider — switch to stable UUIDs

**Files:**
- Modify: `crates/flotilla-core/src/providers/workspace/cmux.rs`

- [ ] **Step 1: Update `parse_workspaces` to read UUID**

In `crates/flotilla-core/src/providers/workspace/cmux.rs`, replace `parse_workspaces` (lines 52-68):

```rust
    fn parse_workspaces(output: &str) -> Result<Vec<(String, Workspace)>, String> {
        let parsed: serde_json::Value = serde_json::from_str(output).map_err(|e| e.to_string())?;
        let workspaces = parsed["workspaces"].as_array().ok_or("cmux list-workspaces: response missing 'workspaces' array")?;
        Ok(workspaces
            .iter()
            .filter_map(|ws| {
                let ws_ref = ws["id"].as_str()?.to_string();
                let name = ws["title"].as_str().unwrap_or("").to_string();
                Some((ws_ref, Workspace { name, correlation_keys: vec![], attachable_set_id: None }))
            })
            .collect())
    }
```

Add a new helper to parse `--id-format both` output (returns a map of positional ref → UUID):

```rust
    fn parse_workspaces_both(output: &str) -> Result<HashMap<String, String>, String> {
        let parsed: serde_json::Value = serde_json::from_str(output).map_err(|e| e.to_string())?;
        let workspaces = parsed["workspaces"].as_array().ok_or("cmux list-workspaces: response missing 'workspaces' array")?;
        Ok(workspaces
            .iter()
            .filter_map(|ws| {
                let ws_ref = ws["ref"].as_str()?.to_string();
                let id = ws["id"].as_str()?.to_string();
                Some((ws_ref, id))
            })
            .collect())
    }
```

- [ ] **Step 2: Update `list_workspaces` to pass `--id-format uuids`**

In the `list_workspaces` method, change the cmux command from:

```rust
let output = match self.cmux_cmd(&["--json", "list-workspaces", "--window", &window_ref]).await {
```

to:

```rust
let output = match self.cmux_cmd(&["--json", "--id-format", "uuids", "list-workspaces", "--window", &window_ref]).await {
```

- [ ] **Step 3: Update `create_workspace` to resolve UUID**

After the workspace is created and `ws_ref` is parsed from `OK workspace:N`, add a follow-up call to resolve the UUID. Find the section near the end of `create_workspace` that returns the result. Before the final `Ok(...)`, insert:

```rust
        // Resolve the positional ref to a stable UUID
        let ws_uuid = {
            let both_output = self.cmux_cmd(&["--json", "--id-format", "both", "list-workspaces", "--workspace", &ws_ref]).await?;
            let ref_to_uuid = Self::parse_workspaces_both(&both_output)?;
            ref_to_uuid.get(&ws_ref).cloned().ok_or_else(|| {
                format!("cmux: could not resolve UUID for {ws_ref} after workspace creation")
            })?
        };
```

Then update the return and log to use `ws_uuid` instead of `ws_ref`:

```rust
        info!(workspace = %config.name, %ws_uuid, "cmux: workspace ready");
        Ok((ws_uuid, Workspace { name: config.name.clone(), correlation_keys: vec![], attachable_set_id: None }))
```

Also update all `--workspace` arguments within `create_workspace` that reference `&ws_ref` — these still use the positional ref during creation (before UUID resolution), which is fine since cmux accepts both formats.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --workspace --locked`
Expected: PASS

- [ ] **Step 5: Add unit test for `parse_workspaces` with UUID format**

Add to the tests module in `cmux.rs`:

```rust
    #[test]
    fn parse_workspaces_reads_uuid_from_id_field() {
        let json = r#"{"workspaces": [
            {"id": "CBC42D5B-AFAE-46BA-A5DB-386D13DA5A40", "title": "Main", "selected": true, "pinned": false, "index": 0},
            {"id": "367CC5E4-0C9B-4559-9D97-D6358900ECCA", "title": "Feature", "selected": false, "pinned": false, "index": 1}
        ]}"#;
        let workspaces = CmuxWorkspaceManager::parse_workspaces(json).unwrap();
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].0, "CBC42D5B-AFAE-46BA-A5DB-386D13DA5A40");
        assert_eq!(workspaces[0].1.name, "Main");
        assert_eq!(workspaces[1].0, "367CC5E4-0C9B-4559-9D97-D6358900ECCA");
        assert_eq!(workspaces[1].1.name, "Feature");
    }

    #[test]
    fn parse_workspaces_both_maps_ref_to_uuid() {
        let json = r#"{"workspaces": [
            {"ref": "workspace:1", "id": "ABC-UUID", "title": "Main", "selected": true, "pinned": false, "index": 0}
        ]}"#;
        let map = CmuxWorkspaceManager::parse_workspaces_both(json).unwrap();
        assert_eq!(map.get("workspace:1"), Some(&"ABC-UUID".to_string()));
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p flotilla-core parse_workspaces --locked`
Expected: PASS for the new unit tests. Note: existing replay-based tests may fail — these need re-recording (Task 9).

- [ ] **Step 7: Commit**

```
feat: switch cmux workspace provider to stable UUIDs
```

---

### Task 6: zellij provider — switch to session:tab_id, remove state files

**Files:**
- Modify: `crates/flotilla-core/src/providers/workspace/zellij.rs`

- [ ] **Step 1: Rewrite `list_workspaces` to use `list-tabs --json`**

Replace the `list_workspaces` method body (lines 157-193):

```rust
    async fn list_workspaces(&self) -> Result<Vec<(String, Workspace)>, String> {
        let output = self.zellij_action(&["list-tabs", "--json"]).await?;
        let tabs: Vec<serde_json::Value> = serde_json::from_str(&output).map_err(|e| format!("zellij list-tabs: {e}"))?;

        let session = self.session_name()?;

        let workspaces = tabs
            .iter()
            .filter_map(|tab| {
                let tab_id = tab["tab_id"].as_u64()?;
                let name = tab["name"].as_str()?.to_string();
                let ws_ref = format!("{session}:{tab_id}");
                Some((ws_ref, Workspace { name, correlation_keys: vec![], attachable_set_id: None }))
            })
            .collect();

        Ok(workspaces)
    }
```

- [ ] **Step 2: Update `create_workspace` to use tab_id from stdout**

In `create_workspace`, replace the `new-tab` call and the ws_ref construction. The current code creates the tab and uses `config.name` as the ws_ref. Change to:

```rust
        // Create new tab — stdout returns the tab_id
        let tab_id_str = self.zellij_action(&["new-tab", "--name", &config.name, "--cwd", &working_dir]).await?;
        let tab_id = tab_id_str.trim();
        let session = self.session_name()?;
        let ws_ref = format!("{session}:{tab_id}");
```

Update the return at the end of the method to use `ws_ref` instead of `config.name.clone()`:

```rust
        Ok((ws_ref, Workspace { name: config.name.clone(), correlation_keys: vec![], attachable_set_id: None }))
```

- [ ] **Step 3: Update `select_workspace` to use `go-to-tab-by-id`**

Replace the `select_workspace` method:

```rust
    async fn select_workspace(&self, ws_ref: &str) -> Result<(), String> {
        let tab_id = ws_ref.rsplit_once(':').map(|(_, id)| id).ok_or_else(|| format!("invalid zellij ws_ref: {ws_ref}"))?;
        info!(%ws_ref, %tab_id, "zellij: switching to tab by id");
        self.zellij_action(&["go-to-tab-by-id", tab_id]).await?;
        Ok(())
    }
```

- [ ] **Step 4: Remove state file code**

Delete from `ZellijWorkspaceManager`:
- `ZellijState` struct and `TabState` struct (lines 27-36)
- `state_dir` field from the struct
- `state_path()`, `load_state()`, `save_state()` methods
- All state-related code in `list_workspaces` (loading, pruning, enrichment) and `create_workspace` (saving state)
- Remove `state_dir` parameter from `new()` and `with_session_name()` constructors

Update all callers that pass `state_dir` — search for `ZellijWorkspaceManager::new` and `ZellijWorkspaceManager::with_session_name`.

Delete associated tests:
- `state_path_contains_session_name`
- `load_state_returns_default_for_missing_file`
- `toml_serialization_round_trip`
- `corrupt_toml_fails_deserialization`
- `state_serialization_format`
- `prune_retains_only_live_tabs`, `prune_empty_state_is_noop`, `prune_all_live_removes_nothing`

Remove unused imports (`HashMap`, `HashSet`, `SystemTime`, `Serialize`, `Deserialize`, `toml`, `DaemonHostPath` if no longer needed).

- [ ] **Step 5: Verify compilation**

Run: `cargo check --workspace --locked`
Expected: PASS — fix any remaining compilation errors from removed state_dir parameter.

- [ ] **Step 6: Commit**

```
feat: switch zellij provider to session:tab_id identity

Remove TOML state files — working directory enrichment is no longer
needed since directories was removed from Workspace.
```

---

### Task 7: tmux provider — switch to start_time:session:@window_id, remove state files

**Files:**
- Modify: `crates/flotilla-core/src/providers/workspace/tmux.rs`

- [ ] **Step 1: Rewrite `list_workspaces` to use window IDs**

Replace the `list_workspaces` method body:

```rust
    async fn list_workspaces(&self) -> Result<Vec<(String, Workspace)>, String> {
        let session = self.session_name().await?;
        let start_time = self.tmux_cmd(&["display-message", "-p", "#{start_time}"]).await?;
        let output = self
            .tmux_cmd(&["list-windows", "-F", "#{window_id}\t#{window_name}"])
            .await?;

        let workspaces = output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|line| {
                let (window_id, name) = line.split_once('\t')?;
                let ws_ref = format!("{start_time}:{session}:{window_id}");
                Some((ws_ref, Workspace { name: name.to_string(), correlation_keys: vec![], attachable_set_id: None }))
            })
            .collect();

        Ok(workspaces)
    }
```

- [ ] **Step 2: Update `create_workspace` to capture window ID**

Replace the window creation section. Instead of:

```rust
self.tmux_cmd(&["new-window", "-n", &config.name, "-c", &working_dir]).await?;
```

Use:

```rust
        let window_id = self
            .tmux_cmd(&["new-window", "-n", &config.name, "-c", &working_dir, "-P", "-F", "#{window_id}"])
            .await?;
        let session = self.session_name().await?;
        let start_time = self.tmux_cmd(&["display-message", "-p", "#{start_time}"]).await?;
        let ws_ref = format!("{start_time}:{session}:{window_id}");
```

Update the return at the end:

```rust
        Ok((ws_ref, Workspace { name: config.name.clone(), correlation_keys: vec![], attachable_set_id: None }))
```

- [ ] **Step 3: Update `select_workspace` to use window ID**

Replace the `select_workspace` method:

```rust
    async fn select_workspace(&self, ws_ref: &str) -> Result<(), String> {
        let window_id = ws_ref.rsplit_once(':').map(|(_, id)| id).ok_or_else(|| format!("invalid tmux ws_ref: {ws_ref}"))?;
        info!(%ws_ref, %window_id, "tmux: switching to window by id");
        self.tmux_cmd(&["select-window", "-t", window_id]).await?;
        Ok(())
    }
```

- [ ] **Step 4: Update `binding_scope_prefix`**

Now that `list_workspaces` queries start_time and session_name, update the `binding_scope_prefix` method (added in Task 4):

```rust
    fn binding_scope_prefix(&self) -> String {
        // Cannot compute prefix synchronously without cached values.
        // Return empty to avoid blocking — pruning will be conservative.
        // TODO: cache start_time and session_name at probe time for proper scoping.
        String::new()
    }
```

Note: `binding_scope_prefix` is a sync method but `session_name()` and the `start_time` query are async. For now, return empty (conservative — won't prune). A follow-up can cache these values at provider probe time.

- [ ] **Step 5: Remove state file code**

Delete from `TmuxWorkspaceManager`:
- `TmuxState` struct and `WindowState` struct
- `state_dir` field
- `state_path()`, `load_state()`, `save_state()` methods
- All state-related code in `list_workspaces` and `create_workspace`
- Remove `state_dir` from `new()` constructor

Delete associated tests (same pattern as zellij — state_path, load_state, toml, prune tests).

Remove unused imports.

- [ ] **Step 6: Verify compilation**

Run: `cargo check --workspace --locked`
Expected: PASS

- [ ] **Step 7: Commit**

```
feat: switch tmux provider to start_time:session:@window_id identity

Remove TOML state files.
```

---

### Task 8: Rewrite `select_existing_workspace` via binding lookup

**Files:**
- Modify: `crates/flotilla-core/src/executor/workspace.rs:66-116, 145-166`

- [ ] **Step 1: Rewrite `select_existing_workspace`**

Replace the stubbed method with the binding-based lookup:

```rust
    fn find_existing_workspace_ref(&self, provider_name: &str, target_host: &HostName, checkout_path: &Path) -> Option<String> {
        let store = self.attachable_store.lock().ok()?;
        let checkout = HostPath::new(target_host.clone(), checkout_path.to_path_buf());
        let set_ids = store.sets_for_checkout(&checkout);
        for set_id in set_ids {
            if let Some(ws_ref) = store.lookup_workspace_ref_for_set("workspace_manager", provider_name, &set_id) {
                return Some(ws_ref);
            }
        }
        None
    }
```

- [ ] **Step 2: Update `attach_prepared_workspace` to use the new method**

Replace the call in `attach_prepared_workspace` (line 71):

From:
```rust
        if prepared.target_host == *self.local_host && self.select_existing_workspace(ws_mgr.as_ref(), &prepared.checkout_path).await {
            return Ok(());
        }
```

To:
```rust
        if let Some(ws_ref) = self.find_existing_workspace_ref(provider_name, &prepared.target_host, &prepared.checkout_path) {
            info!(%ws_ref, "found existing workspace via binding, selecting");
            match ws_mgr.select_workspace(&ws_ref).await {
                Ok(()) => return Ok(()),
                Err(err) => warn!(err = %err, %ws_ref, "failed to select existing workspace, will create new"),
            }
        }
```

Note: this now works for both local and remote workspaces — the `prepared.target_host == *self.local_host` guard is removed.

- [ ] **Step 3: Remove the old `select_existing_workspace` method**

Delete the stubbed `select_existing_workspace` method entirely.

- [ ] **Step 4: Verify compilation and run tests**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Run: `cargo test --workspace --locked`
Expected: PASS

- [ ] **Step 5: Commit**

```
feat: rewrite workspace selection to use binding-based lookup

Works for both local and remote workspaces. Falls back to
creating a new workspace if selection fails.
```

---

### Task 9: Stale binding pruning in refresh

**Files:**
- Modify: `crates/flotilla-core/src/refresh.rs:275-320`

- [ ] **Step 1: Update `project_attachable_data` signature**

The function needs access to the workspace manager for `binding_scope_prefix()`. Change the function to also accept the workspace info. In `refresh.rs`, update the function signature and caller.

First, update `project_attachable_data` to accept the scope prefix and provider name as parameters (computed by the caller from the registry):

```rust
fn project_attachable_data(
    pd: &mut ProviderData,
    registry: &ProviderRegistry,
    attachable_store: &SharedAttachableStore,
) {
```

The caller already has the registry. Inside `project_attachable_data`, after the workspace binding loop, add pruning.

- [ ] **Step 2: Add pruning logic**

In `project_attachable_data`, after the existing workspace binding loop (after line 290), add:

```rust
    // Prune stale workspace bindings within the provider's declared scope.
    if let Some((desc, ws_mgr)) = registry.workspace_managers.preferred_with_desc() {
        let provider_name = &desc.implementation;
        let scope_prefix = ws_mgr.binding_scope_prefix();
        let live_ws_refs: std::collections::HashSet<&str> = pd.workspaces.keys().map(|s| s.as_str()).collect();

        let stale_refs: Vec<String> = store
            .registry()
            .bindings
            .iter()
            .filter(|b| {
                b.provider_category == "workspace_manager"
                    && b.provider_name == *provider_name
                    && b.object_kind == crate::attachable::BindingObjectKind::AttachableSet
                    && b.external_ref.starts_with(&scope_prefix)
                    && !live_ws_refs.contains(b.external_ref.as_str())
            })
            .map(|b| b.external_ref.clone())
            .collect();

        if !stale_refs.is_empty() {
            // Need mutable access — drop immutable guard and re-acquire
            drop(store);
            if let Ok(mut store) = attachable_store.lock() {
                for stale_ref in &stale_refs {
                    tracing::info!(external_ref = %stale_ref, provider = %provider_name, "pruning stale workspace binding");
                    store.remove_binding_object("workspace_manager", provider_name, crate::attachable::BindingObjectKind::AttachableSet, stale_ref);
                }
                if let Err(err) = store.save() {
                    tracing::warn!(err = %err, "failed to save after pruning stale workspace bindings");
                }
            }
            return; // store was dropped and re-acquired; remaining projections already done above
        }
    }
```

Note: the lock dance (drop immutable, re-acquire mutable) is needed because the store is borrowed immutably for the read phase. An alternative is to take a mutable lock from the start — review the existing code to determine if that's safe.

Actually, looking at the code more carefully, `attachable_store.lock()` returns a `MutexGuard` which gives both `&` and `&mut` access. The issue is that we need to collect the stale refs while borrowing the registry immutably, then mutate. The simplest approach is to collect first, then mutate:

```rust
    // Prune stale workspace bindings within the provider's declared scope.
    if let Some((desc, ws_mgr)) = registry.workspace_managers.preferred_with_desc() {
        let provider_name = &desc.implementation;
        let scope_prefix = ws_mgr.binding_scope_prefix();
        let live_ws_refs: std::collections::HashSet<&str> = pd.workspaces.keys().map(|s| s.as_str()).collect();

        let stale_refs: Vec<String> = store
            .registry()
            .bindings
            .iter()
            .filter(|b| {
                b.provider_category == "workspace_manager"
                    && b.provider_name == *provider_name
                    && b.object_kind == crate::attachable::BindingObjectKind::AttachableSet
                    && b.external_ref.starts_with(&scope_prefix)
                    && !live_ws_refs.contains(b.external_ref.as_str())
            })
            .map(|b| b.external_ref.clone())
            .collect();

        if !stale_refs.is_empty() {
            drop(store);
            let Ok(mut store) = attachable_store.lock() else { return };
            for stale_ref in &stale_refs {
                tracing::info!(external_ref = %stale_ref, provider = %provider_name, "pruning stale workspace binding");
                store.remove_binding_object("workspace_manager", provider_name, crate::attachable::BindingObjectKind::AttachableSet, stale_ref);
            }
            if let Err(err) = store.save() {
                tracing::warn!(err = %err, "failed to save after pruning stale workspace bindings");
            }
        }
    }
```

Place this at the end of `project_attachable_data`, after the managed_terminals loop. The function will need restructuring — the `store` lock is taken at the top and used throughout. The pruning must come last so we can drop and re-acquire.

- [ ] **Step 3: Verify compilation and run tests**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Run: `cargo test --workspace --locked`
Expected: PASS

- [ ] **Step 4: Commit**

```
feat: prune stale workspace bindings during refresh

Scoped by provider name and binding_scope_prefix to only prune
bindings the current provider instance is authoritative about.
```

---

### Task 10: Re-record replay fixtures

**Files:**
- Modify: `crates/flotilla-core/src/providers/workspace/fixtures/cmux_*.yaml`
- Modify: `crates/flotilla-core/src/providers/workspace/fixtures/zellij_*.yaml`
- Modify: `crates/flotilla-core/src/providers/workspace/fixtures/tmux_*.yaml`

**This task requires human intervention** — the multiplexers must be running.

- [ ] **Step 1: Re-record cmux fixtures**

Ensure cmux is running. Then:

```bash
REPLAY=record cargo test -p flotilla-core record_replay_create_and_switch -- --ignored 2>&1 || true
REPLAY=record cargo test -p flotilla-core record_replay_list_workspaces -- --ignored 2>&1 || true
```

Review the generated fixture files — ws_refs should now be UUIDs.

- [ ] **Step 2: Re-record zellij fixtures**

Ensure a zellij session is running. Then:

```bash
REPLAY=record cargo test -p flotilla-core record_replay_create_and_switch_workspaces -- zellij --ignored 2>&1 || true
REPLAY=record cargo test -p flotilla-core record_replay_list_workspaces -- zellij --ignored 2>&1 || true
```

Review — ws_refs should be `{session}:{tab_id}` format.

- [ ] **Step 3: Re-record tmux fixtures**

```bash
REPLAY=record cargo test -p flotilla-core record_replay_create_and_switch_workspaces -- tmux --ignored 2>&1 || true
REPLAY=record cargo test -p flotilla-core record_replay_list_workspaces -- tmux --ignored 2>&1 || true
```

Review — ws_refs should be `{start_time}:{session}:@{window_id}` format.

- [ ] **Step 4: Run all tests in replay mode**

```bash
cargo test --workspace --locked
```

Expected: ALL PASS

- [ ] **Step 5: Commit**

```
test: re-record workspace provider replay fixtures for stable IDs
```

---

### Task 11: Final verification

- [ ] **Step 1: Run the full CI gate**

```bash
cargo +nightly-2026-03-12 fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: All pass.

- [ ] **Step 2: Commit any formatting fixes**

```bash
cargo +nightly-2026-03-12 fmt
```
