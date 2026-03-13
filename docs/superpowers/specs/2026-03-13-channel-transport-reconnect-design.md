# ChannelTransport Envelope-Based Reconnection Design

**Date:** 2026-03-13
**Issue:** #302
**Status:** Approved

## Problem

`ChannelTransport` is single-lifecycle — `disconnect()` consumes the underlying mpsc channels, making `reconnect_peer()` fail. This means reconnection/failover paths in `PeerManager` can't be tested with the in-process transport: generation tracking across reconnections, displaced sender retirement, reconnect suppression after Goodbye, and failover resync.

## Approach

Model connection lifecycle as in-band messages over a persistent backbone channel. The physical channel lives for the lifetime of the pair; connect/disconnect are logical operations expressed as envelopes.

```rust
enum ChannelEnvelope {
    Connected,
    Packet(PeerWireMessage),
    Disconnected,
}
```

This gives the same semantics as TCP/TLS/SSH: when one side disconnects, the other side's receiver closes and its transport transitions to `Disconnected`. Reconnection creates a new session over the same backbone.

## Design

### Backbone and session channels

Each `ChannelTransport` pair shares a persistent bidirectional backbone via `mpsc<ChannelEnvelope>`. The backbone never closes — it outlives individual connection sessions.

On top of the backbone, each `connect()` call creates a fresh session channel (`mpsc<PeerWireMessage>`) and spawns a forwarding task that reads `Packet` envelopes from the backbone and writes the inner `PeerWireMessage` to the session channel.

### Internal structure

```rust
pub struct ChannelTransport {
    local_name: HostName,
    remote_name: HostName,
    // Shared with forwarding task — task sets Disconnected on remote drop
    status: Arc<std::sync::Mutex<PeerConnectionStatus>>,
    // Backbone — persistent for the lifetime of the pair
    backbone_tx: mpsc::Sender<ChannelEnvelope>,
    backbone_rx: Arc<std::sync::Mutex<Option<mpsc::Receiver<ChannelEnvelope>>>>,
    // Session — created fresh per connect() cycle
    session_tx: Option<mpsc::Sender<PeerWireMessage>>,
    session_rx: Option<mpsc::Receiver<PeerWireMessage>>,
    // Cancellation signal — dropped to tell the forwarding task to stop
    cancel_tx: Option<oneshot::Sender<()>>,
    // Forwarding task handle — awaited on disconnect for clean shutdown
    task_handle: Option<tokio::task::JoinHandle<()>>,
}
```

**Mutex types:** Both `status` and `backbone_rx` use `std::sync::Mutex` (not `tokio::sync::Mutex`). This is required because the forwarding task must return `backbone_rx` on all exit paths including cancellation, and `std::sync::Mutex::lock()` is safe to call in non-async contexts like `Drop` impls. The locks are held only briefly (status read/write, receiver take/put).

- `backbone_tx` / `backbone_rx`: The backbone receiver lives in `Arc<std::sync::Mutex<Option<...>>>`. The forwarding task takes it on start and returns it on exit (all paths).
- `session_tx` / `session_rx`: Fresh per connect cycle. `subscribe()` takes `session_rx`. The forwarding task writes to a clone of `session_tx`.
- `cancel_tx`: A `oneshot::Sender<()>` dropped by `disconnect()` to signal the forwarding task to exit gracefully. The task selects on this alongside the backbone receiver.
- `status`: Shared with the forwarding task via `Arc<std::sync::Mutex<...>>` so the task can set `Disconnected` when the remote side drops.

### Transport state machine

**`connect()`:**
1. Check status is `Disconnected` (fail otherwise)
2. Take the backbone receiver from the `Arc<std::sync::Mutex<Option<...>>>`
3. Drain any stale envelopes from the backbone receiver (leftover `Connected`, `Packet`, `Disconnected` from previous sessions) — discard them
4. Create a fresh session channel (`mpsc<PeerWireMessage>`)
5. Create a `oneshot` cancellation channel
6. Send `Connected` on the backbone sender
7. Spawn a forwarding task, passing it: backbone receiver, session sender clone, cancel receiver, shared status, shared backbone_rx arc
8. Store session_rx, cancel_tx, and task handle
9. Set status to `Connected`

**`disconnect()`:**
1. Send `Disconnected` on the backbone sender (best-effort — ignore errors if backbone is full)
2. Drop `cancel_tx` — signals the forwarding task to exit gracefully
3. `.await` the task handle — ensures the task has fully exited and returned the backbone receiver to the `Arc<std::sync::Mutex<Option<...>>>` before `disconnect()` returns
4. Drop session_tx and session_rx (subscriber gets `None`)
5. Set status to `Disconnected`
6. Can be called again — backbone is still alive

**`subscribe()`:** Returns the session receiver. One-shot per connect cycle, same as before.

**`sender()`:** Returns a `ChannelSender` that wraps the backbone sender, sending `Packet(msg)` envelopes. Returns `None` when not `Connected`. Note: previously returned `Arc<dyn PeerSender>` references remain functional after disconnect — they can still enqueue to the backbone (matching TCP semantics). The status gate only affects new `sender()` calls.

### Forwarding task

The forwarding task bridges the backbone to the session channel. It owns the backbone receiver for the duration of its execution and returns it on all exit paths.

The task runs a `tokio::select!` loop with two branches:
- **Backbone receive:** reads the next `ChannelEnvelope`
- **Cancel signal:** the `oneshot::Receiver<()>` completes when `disconnect()` drops `cancel_tx`

**Envelope handling:**
- `Packet(msg)` — forwards `msg` to the session sender. If the session sender is closed (subscriber dropped), ignore the error.
- `Disconnected` — the remote side disconnected. Drops the session sender (subscriber gets `None`), sets local status to `Disconnected` via the shared `Arc<std::sync::Mutex<...>>`, returns the backbone receiver, and exits.
- `Connected` — ignored (no-op). This can arrive when the remote side reconnects while the local side is still connected. The local side doesn't need to act on it.
- `None` (backbone closed) — should not happen in normal operation (backbone is persistent). Treat as fatal: set status to `Disconnected`, return backbone receiver, exit.

**On cancel signal (local disconnect):**
- Return the backbone receiver to the `Arc<std::sync::Mutex<Option<...>>>` and exit. The caller (`disconnect()`) handles session cleanup and status.

**Backbone receiver recovery:** On all exit paths (remote disconnect, cancel signal, backbone closed), the task puts the backbone receiver back into the shared `Arc<std::sync::Mutex<Option<...>>>` before returning. This uses `std::sync::Mutex::lock()` which is safe in both async and sync contexts.

### Stale envelope handling

When `connect()` takes the backbone receiver, it drains any queued envelopes before spawning the forwarding task. This handles the case where messages from a previous session are still in the backbone buffer (e.g., the remote side sent `Disconnected` + `Connected` + `Packet` while the local side was disconnected). Draining ensures the forwarding task starts with a clean backbone.

### Backbone backpressure

The backbone uses a bounded channel (`CHANNEL_BUFFER = 256`). If the remote side is disconnected and the local side keeps sending, the backbone will fill up and `send()` will block. This is acceptable for testing scenarios — a test that sends while the remote is disconnected will deadlock, which is a test authoring bug, not a transport bug. In production usage, the same behavior would apply (analogous to a TCP send buffer filling when the remote is unresponsive). `ChannelSender` holds a clone of `backbone_tx`, so held `Arc<dyn PeerSender>` references can still enqueue to the backbone after local disconnect — again matching TCP semantics where sends succeed until the kernel buffer fills.

### ChannelSender changes

`ChannelSender` wraps the backbone sender instead of a session sender:

```rust
pub struct ChannelSender {
    tx: tokio::sync::Mutex<Option<mpsc::Sender<ChannelEnvelope>>>,
}
```

- `send(msg)` — sends `Packet(msg)` on the backbone
- `retire(reason)` — sends `Packet(Goodbye { reason })` then takes the sender

### Factory function

`channel_transport_pair()` creates two backbone channels (A→B and B→A) and wires them into two `ChannelTransport` instances, same as before but with the new internal structure.

## Scope

### In scope
- Rewrite `ChannelTransport` internals to backbone + session architecture
- Update `channel_transport_pair()` to create backbone channels
- `ChannelSender` wraps backbone sender with `Packet` envelope
- Forwarding task with remote disconnect detection
- Update existing 14 unit tests (`reconnect_after_disconnect` flips from failure to success)
- New reconnection unit tests

### New unit tests
- `reconnect_after_disconnect_succeeds` — disconnect, reconnect, verify send/receive works
- `remote_disconnect_closes_local_receiver` — A disconnects, B's subscriber gets `None`
- `remote_disconnect_transitions_status` — A disconnects, B's status becomes `Disconnected`
- `reconnect_after_remote_disconnect` — A disconnects, B detects, both reconnect, bidirectional messaging resumes
- `multiple_reconnect_cycles` — connect/disconnect/connect several times, verify each cycle works

### Out of scope
- Reconnection integration tests via TestNetwork (needs TestNetwork changes)
- FailureTransport wrapper / failure injection
- PeerManager reconnection integration tests (future phase)

## Success criteria
- All existing `channel_tests` pass unchanged (TestNetwork doesn't use reconnection)
- Existing unit tests pass (with `reconnect_after_disconnect` updated)
- New reconnection unit tests pass
- `PeerTransport` contract fully satisfied including `reconnect_peer()` path
- No production behavior changes for single-lifecycle usage
