# CLI, Multi-Host Commands, and Integration Testing

**Date**: 2026-03-13
**Status**: Draft

## Motivation

Flotilla's multi-host peering (SSH transport, follower mode, peer relay) needs end-to-end validation across realistic network topologies — different hostnames, heterogeneous tool installations, jump boxes, VPNs. This requires:

1. A rich CLI surface that can query and control flotilla programmatically (not just the TUI)
2. Docker Compose test infrastructure simulating real deployment scenarios
3. A pytest harness driving assertions against the CLI

The CLI is a first-class goal (scriptable, composable, useful to users) and also the primary interface for integration tests.

## CLI Design

### Output Formatting

All commands support two output modes:

- **Human-friendly** (default): tables, concise status lines
- **JSON** (`--json` flag): structured output for scripting and test assertions

A shared output layer renders the same data in both formats, established as a pattern for all commands.

### Command Grammar

Commands follow a `<noun> [scope] <verb>` grammar. Scoping narrows context:

```
flotilla work                              # all work items
flotilla repo <path_or_slug> work          # work for one repo
flotilla host <host> work                  # work from one host
```

The `host <host>` prefix routes commands to a remote host's daemon via the peer protocol. Any command can be remote-targeted this way:

```
flotilla host feta repo add /path/to/repo
```

`path_or_slug` matching: full path, repo name, or unique substring.

### Query Commands (one-shot, request/response)

| Command | Description |
|---------|-------------|
| `flotilla status` | High-level overview: repos, health, peers |
| `flotilla host providers` | Host-level provider discovery (binaries, sockets, auth) |
| `flotilla repo [path_or_slug] providers` | Provider instances active for a specific repo |
| `flotilla repo [path_or_slug]` | Repo overview (branches, PRs, sessions) |
| `flotilla work` | Correlated work items across repos |

### Control Commands (mutating, block until result)

| Command | Description |
|---------|-------------|
| `flotilla refresh [repo]` | Trigger refresh (omit repo = all) |
| `flotilla repo add <path>` | Track a new repo |
| `flotilla repo remove <path_or_slug>` | Stop tracking a repo |
| `flotilla repo <path_or_slug> checkout <branch> [path]` | Create checkout (path optional) |
| `flotilla checkout <path> remove` | Remove a checkout |

Control commands send `Command` variants through `execute()`, block until `CommandFinished`, and exit with appropriate exit codes.

### Multi-Host Commands

| Command | Description |
|---------|-------------|
| `flotilla host list` | All hosts, connection status, providers |
| `flotilla host <host> status` | Detailed single host view |
| `flotilla host <host> providers` | Remote host provider discovery |
| `flotilla topology` | Peering topology (table, `--json`, `--dot` for Graphviz) |

### Streaming Commands

| Command | Description |
|---------|-------------|
| `flotilla watch` | Stream daemon events (`--json`, filterable) |
| `flotilla host <host> watch` | Events from specific host |
| `flotilla repo <path_or_slug> watch` | Events for specific repo |
| `flotilla checkout <path> watch` | Events for specific checkout |

`watch` uses the existing `DaemonEvent` subscription. The scope prefix applies a filter — same stream, narrower view.

## Docker Infrastructure

### Image Strategy

**Base + role layers:**

- `flotilla-base`: Debian slim, multi-stage Rust build, SSH server, flotilla binary, minimal user setup
- Role images `FROM flotilla-base`, add per-role tooling
- Build inside the container (multi-stage) for correctness — no cross-compile needed

**Roles:**

| Role | Providers | Coding Agent | Session Manager | Notes |
|------|-----------|-------------|-----------------|-------|
| `workstation` | Full (`gh`, all providers) | claude | tmux + zellij, shpool | Leader, workspace transfer testing |
| `follower-codex` | Local only (VCS, workspace) | codex | shpool | Persistent sessions |
| `follower-gemini` | Local only (VCS, workspace) | gemini | No shpool | Direct SSH spawn |
| `jumpbox` | Minimal (SSH only) | None | None | Follower mode, relay only |

### Topologies

**Topology 1 — Minimal (2-node direct SSH):**

```
[node-a: workstation] ←SSH→ [node-b: follower]
```

Single Docker network. Validates basic peering, CLI commands, event streaming.

**Topology 2 — Hub-spoke (1 workstation + 2 followers):**

```
                    [homelab-1: follower-codex]
                   /
[workstation] ←SSH→
                   \
                    [homelab-2: follower-gemini]
```

Single Docker network. Followers peer with workstation only, not each other. Tests:
- Provider heterogeneity (different coding agents, shpool vs no-shpool)
- Work correlation across 3 hosts
- Followers receive service data (PRs, issues) without having `gh`
- Session persistence: shpool (homelab-1) survives disconnect, direct spawn (homelab-2) doesn't
- Workspace transfer: disconnect tmux on workstation, respawn in zellij
- Resilience: kill workstation → followers detect → restart → resync

**Topology 3 — Jump box (3-node bastion routing):**

```
[vpn-net]                          [homelab-net]
[workstation] ←SSH→ [jumpbox] ←SSH→ [homelab]
```

Two Docker networks. Workstation cannot reach homelab directly.

Tests:
- Network isolation verified (workstation cannot SSH to homelab directly)
- Peer relay: data flows workstation → jumpbox → homelab (and back)
- `flotilla topology --dot` shows full chain
- `flotilla work` on workstation includes transitive items from homelab
- Remote commands route through jumpbox: `flotilla host homelab repo add <path>`
- Partition: kill jumpbox → workstation loses homelab visibility → restart → recovery

## Test Harness

**Language**: Python (pytest)

**Structure:**
- Fixtures for Docker Compose lifecycle (up/down per topology)
- Helper to SSH into a node and run `flotilla <command> --json`, returning parsed JSON
- Parameterized tests across topologies where applicable
- Separate test modules per topology

**Example test pattern:**

```python
def test_peer_connectivity(minimal_topology):
    result = flotilla(minimal_topology["node-a"], "host list --json")
    peers = result["hosts"]
    assert any(h["name"] == "node-b" and h["status"] == "connected" for h in peers)
```

## Issue Breakdown

### Issue 1: CLI output formatting infrastructure

Add `--json` flag and shared output layer. Retrofit existing `status` and `watch` subcommands. Establish pattern for all future commands.

**Labels**: `enhancement`, `infrastructure`

### Issue 2: CLI query commands

`status`, `host providers`, `repo [path_or_slug] providers`, `repo [path_or_slug]`, `work`. All one-shot, all support `--json`.

**Labels**: `enhancement`
**Blocked by**: Issue 1

### Issue 3: CLI control commands

`refresh`, `repo add/remove`, `repo checkout`, `checkout remove`. Remote targeting via `host <host>` prefix. `path_or_slug` matching.

**Labels**: `enhancement`
**Blocked by**: Issue 1

### Issue 4: CLI multi-host commands

`host list`, `host <host> status`, `host <host> providers`, `topology` (with `--dot`), `watch` scoping.

**Labels**: `enhancement`
**Blocked by**: Issue 1

### Issue 5: Dockerfile base + role images

`flotilla-base` (Debian slim, multi-stage build, SSH), role images per topology need. No external registry.

**Labels**: `infrastructure`, `testing`

### Issue 6: Minimal topology — 2-node direct SSH + pytest harness

First end-to-end validation. Pytest skeleton with compose fixtures, SSH helpers, JSON assertion utils.

**Labels**: `testing`, `infrastructure`
**Blocked by**: Issues 1–4, 5

### Issue 7: Hub-spoke topology — 1 workstation + 2 followers

Provider heterogeneity, coding agent diversity (claude/codex/gemini), session persistence (shpool vs direct), workspace transfer (tmux → zellij), resilience.

**Labels**: `testing`
**Blocked by**: Issue 6

### Issue 8: Jump box topology — 3-node bastion routing

Network isolation, peer relay, transitive data flow, remote command routing, partition/recovery.

**Labels**: `testing`
**Blocked by**: Issue 6
