# Workspace Manager: Multi-Checkout Gap

## Current State

A workspace manager (cmux, tmux, zellij) reports its workspaces with a list of directories. Each directory becomes a `CorrelationKey::CheckoutPath`, which the correlation engine uses to merge items into groups.

When a workspace references multiple checkout paths, those paths can pull multiple distinct checkouts into a single correlation group. The resulting `WorkItem` can only represent one checkout (one `worktree_idx`). Today we pick the first checkout encountered and discard others.

This means: if a user has a cmux workspace with panes open in two different worktrees, only one worktree's data appears on the correlated row.

## Ideal Workspace Manager

A workspace manager that understands its purpose would:

1. **Know which checkout is primary** -- a workspace is "about" one branch/worktree, even if it has panes open elsewhere (e.g. the main repo for reference).
2. **Provide event streams or sync RPCs** -- report workspace state changes in real time rather than requiring polling with fixed sleeps.
3. **Report readiness** -- signal when panes/surfaces are ready to receive input, eliminating the need for fixed-delay sleeps during creation.

## Current Adapters

None of cmux, tmux, or zellij directly model the concept of a "primary working directory" for a workspace. We infer it from the directory list. This is a fundamental mismatch between the workspace manager abstraction and the tools available today.

## Design Pressure

When building new workspace manager integrations or improving existing ones:

- Prefer providers that can distinguish primary vs auxiliary directories.
- Push for event-based or callback-based readiness signals over fixed sleeps.
- Document gaps between the ideal contract and what the provider actually supports, so we have ammunition to request improvements upstream or in our own adapters.
