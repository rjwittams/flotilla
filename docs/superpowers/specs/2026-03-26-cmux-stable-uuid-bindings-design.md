# cmux Stable UUID Bindings

## Problem

cmux workspace refs (`workspace:N`) are positional and get reused when workspaces are destroyed and recreated. The attachable binding system uses these refs as keys, so a stale binding from a deleted workspace can match a newly created workspace that happens to reuse the same ref number. This causes workspaces to be associated with the wrong repo through the correlation engine.

**Observed symptom:** The cleat repo's main checkout was associated with attachable set `cd72eaf4-` whose cmux binding pointed to `workspace:5` — a flotilla worktree workspace ("meta-agents-sdlc-streamlining"). The binding was created when workspace:5 was originally a cleat workspace; after that workspace was destroyed and cmux reused the ref for a flotilla workspace, the stale binding persisted.

## Solution

Switch the cmux workspace provider to use cmux's stable UUIDs as the canonical workspace identity. cmux exposes UUIDs via `--id-format uuids` (or `--id-format both` for UUID + positional ref). UUIDs are never reused, eliminating the stale binding collision.

## Scope

Narrow fix to the cmux provider only. The broader attachable set lifecycle (stale binding cleanup, orphaned sets, validation during refresh) is out of scope — to be addressed separately.

## Changes

### `CmuxWorkspaceManager` (cmux.rs)

**`list_workspaces()`:** Pass `--id-format uuids` to the `list-workspaces` command. Update `parse_workspaces()` to read the `id` field (UUID) instead of `ref` as the ws_ref.

**`create_workspace()`:** The `new-workspace` command always returns `OK workspace:N` regardless of id-format flags. After creation, issue a follow-up `list-workspaces --id-format both` call, match by the returned positional ref to find the UUID, and return the UUID as the ws_ref.

**`select_workspace()`:** No change — cmux accepts UUIDs for `--workspace` arguments.

**`parse_workspaces()`:** Read `id` field instead of `ref`.

### Downstream (no changes)

The orchestrator, binding system, refresh, and correlation all treat ws_ref as an opaque string. Swapping positional refs for UUIDs requires no changes outside the cmux provider.

### Migration

None. We are in a no-backwards-compat phase. Existing bindings keyed on `workspace:N` become dead entries (they won't match any workspace from `list_workspaces` since that now returns UUIDs). New bindings use UUIDs. Orphaned old bindings are harmless and can be cleaned up as part of future lifecycle work.

### Tests

Update existing cmux replay fixtures to reflect `--id-format uuids`/`--id-format both` in commands and UUID-format responses. Verify parsed ws_ref values are UUIDs.
