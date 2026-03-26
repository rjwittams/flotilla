# Stable Workspace Identity for cmux and zellij

## Problem

Workspace manager bindings use workspace refs as keys in the attachable registry. Both cmux and zellij use unstable identifiers that can be reused or change, causing stale bindings to associate workspaces with the wrong repo.

**cmux:** Positional refs (`workspace:N`) get reused when workspaces are destroyed and recreated. Observed symptom: the cleat repo's main checkout was associated with a flotilla worktree workspace because the stale binding from a deleted cleat workspace matched a new flotilla workspace that reused the same ref number.

**zellij:** Tab names (used as ws_ref) can be renamed by the user, and duplicate names are possible. The current `query-tab-names` command returns only names with no stable identity.

## Solution

Switch both providers to use stable identifiers as their canonical workspace identity (ws_ref).

- **cmux:** Use cmux's stable UUIDs, exposed via `--id-format uuids`.
- **zellij:** Use `{session_name}:{tab_id}` where `tab_id` comes from the new `list-tabs --json` output. The `tab_id` is stable within a zellij session. Prefixing with the session name prevents cross-session collisions if the daemon connects to a different zellij session.

## Scope

Narrow fix to the cmux and zellij workspace providers. The broader attachable set lifecycle (stale binding cleanup, orphaned sets, validation during refresh) is out of scope.

## Changes

### `CmuxWorkspaceManager` (cmux.rs)

**`list_workspaces()`:** Pass `--id-format uuids` to `list-workspaces`. Update `parse_workspaces()` to read the `id` field (UUID) instead of `ref` as the ws_ref.

**`create_workspace()`:** The `new-workspace` command returns `OK workspace:N` regardless of id-format flags. After creation, issue a follow-up `list-workspaces --id-format both` call, match by the returned positional ref to find the UUID, and return the UUID as the ws_ref.

**`select_workspace()`:** No change — cmux accepts UUIDs for `--workspace` arguments.

### `ZellijWorkspaceManager` (zellij.rs)

**`list_workspaces()`:** Replace `query-tab-names` with `list-tabs --json`. Parse each tab's `tab_id` and `name`. Return ws_ref as `{session_name}:{tab_id}`. Use the `name` field for the `Workspace.name`.

**`create_workspace()`:** `new-tab` now returns the tab_id to stdout. Construct ws_ref as `{session_name}:{tab_id}`.

**`select_workspace()`:** Parse the tab_id from the ws_ref (after the `:`), call `go-to-tab-by-id {tab_id}` instead of `go-to-tab-name`.

### Downstream (no changes)

The orchestrator, binding system, refresh, and correlation all treat ws_ref as an opaque string. Swapping identifiers requires no changes outside the workspace providers.

### Migration

None. We are in a no-backwards-compat phase. Existing bindings keyed on old-format refs become dead entries. New bindings use stable identifiers. Orphaned old bindings are harmless and can be cleaned up as part of future lifecycle work.

### Tests

Update existing cmux and zellij replay fixtures to reflect the new commands and response formats. Verify parsed ws_ref values use the new formats.
