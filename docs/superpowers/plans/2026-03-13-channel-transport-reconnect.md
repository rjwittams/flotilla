# ChannelTransport Reconnection Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite `ChannelTransport` to support reconnection via persistent backbone channels with session envelopes, satisfying the full `PeerTransport` contract including `reconnect_peer()`.

**Architecture:** A persistent backbone `mpsc<ChannelEnvelope>` carries `Connected`, `Packet(PeerWireMessage)`, and `Disconnected` envelopes. Each `connect()` creates a fresh session channel. The forwarding task is spawned lazily in `subscribe()` (not `connect()`) — this means the task only runs when there's an active subscriber, avoiding races when the peer transport is dropped without subscribing. Cooperative shutdown via `oneshot` cancellation ensures clean backbone receiver recovery on all exit paths.

**Tech Stack:** Rust, tokio (mpsc, oneshot, task, select!), std::sync::Mutex for shared state.

**Spec:** `docs/superpowers/specs/2026-03-13-channel-transport-reconnect-design.md`

---

## Chunk 1: Core Implementation and Updated Tests

### File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Rewrite | `crates/flotilla-daemon/src/peer/channel_transport.rs` | Backbone+session transport, ChannelSender, forwarding task |
| No change | `crates/flotilla-daemon/src/peer/mod.rs` | Already exports what we need |
| No change | `crates/flotilla-daemon/src/peer/test_support.rs` | TestNetwork unchanged |
| No change | `crates/flotilla-daemon/src/peer/channel_tests.rs` | Integration tests unchanged |

### Task 1: Rewrite ChannelTransport Internals

**Files:**
- Rewrite: `crates/flotilla-daemon/src/peer/channel_transport.rs`

This task replaces the entire implementation while keeping the same public API (`ChannelTransport`, `ChannelSender`, `channel_transport_pair`).

- [ ] **Step 1: Write the new implementation**

Replace the contents of `crates/flotilla-daemon/src/peer/channel_transport.rs` with the backbone+session architecture. Key changes from the current code:

**Imports** (replace existing):
```rust
use std::sync::Arc;

use async_trait::async_trait;
use flotilla_protocol::{GoodbyeReason, HostName, PeerWireMessage};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::peer::transport::{PeerConnectionStatus, PeerSender, PeerTransport};
```

**New `ChannelEnvelope` enum** (not public — internal to the module):
```rust
enum ChannelEnvelope {
    Connected,
    Packet(PeerWireMessage),
    Disconnected,
}
```

**New `ChannelTransport` struct:**
```rust
/// In-process peer transport using persistent backbone channels with session
/// envelopes. Supports full connect/disconnect/reconnect lifecycle.
///
/// Created in pairs via [`channel_transport_pair`]. A persistent backbone
/// `mpsc<ChannelEnvelope>` carries `Connected`, `Packet`, and `Disconnected`
/// envelopes. Each `connect()` creates a fresh session channel; the forwarding
/// task is spawned lazily in `subscribe()`.
///
/// When the remote side disconnects, the local forwarding task detects the
/// `Disconnected` envelope, closes the session (subscriber gets `None`), and
/// transitions the local status to `Disconnected` — matching TCP/SSH semantics.
pub struct ChannelTransport {
    local_name: HostName,
    remote_name: HostName,
    status: Arc<std::sync::Mutex<PeerConnectionStatus>>,
    // Backbone — persistent for the lifetime of the pair
    backbone_tx: mpsc::Sender<ChannelEnvelope>,
    backbone_rx: Arc<std::sync::Mutex<Option<mpsc::Receiver<ChannelEnvelope>>>>,
    // Session — created fresh per connect() cycle, forwarding task spawned in subscribe()
    session_tx: Option<mpsc::Sender<PeerWireMessage>>,
    session_rx: Option<mpsc::Receiver<PeerWireMessage>>,
    // Forwarding task state — only set after subscribe()
    cancel_tx: Option<oneshot::Sender<()>>,
    task_handle: Option<JoinHandle<()>>,
}
```

**Key design decision — lazy task spawn:** The forwarding task is spawned in `subscribe()`, not `connect()`. This means:
- Tests that create a pair but only connect one side (dropping `_b`) do NOT trigger a forwarding task that races with test assertions when the backbone closes.
- The `connect_all()` path in `PeerManager` calls `connect()` then `sender()` then `subscribe()` in sequence, so the task is spawned at the right time.
- Between `connect()` and `subscribe()`, backbone envelopes queue up and are processed when the task starts.

**`PeerTransport` impl — `connect()`:**
1. Await any existing `task_handle` (ensures previous forwarding task has fully exited and returned backbone_rx)
2. Lock `status`, check it's `Disconnected`, set to `Connecting` then `Connected`
3. Take backbone receiver from `backbone_rx` arc (fail if None — should not happen after step 1)
4. Drain stale envelopes: `while let Ok(_) = backbone_rx_taken.try_recv() {}`
5. Create session channel: `let (session_tx, session_rx) = mpsc::channel(CHANNEL_BUFFER);`
6. Send `Connected` on backbone_tx (best-effort, ignore send error)
7. Put backbone_rx back into arc (subscribe() will take it when spawning the task)
8. Store `session_tx`, `session_rx`
9. Clear old `cancel_tx` and `task_handle`

```rust
async fn connect(&mut self) -> Result<(), String> {
    // Await any previous forwarding task to ensure backbone_rx is returned
    if let Some(handle) = self.task_handle.take() {
        let _ = handle.await;
    }
    self.cancel_tx.take();

    {
        let status = self.status.lock().expect("status lock");
        if *status != PeerConnectionStatus::Disconnected {
            return Err(format!("cannot connect: status is {:?}", *status));
        }
    }

    // Take backbone_rx, drain stale envelopes, put it back
    let mut backbone_rx = self.backbone_rx.lock().expect("backbone lock")
        .take()
        .ok_or("cannot connect: backbone receiver unavailable")?;
    while backbone_rx.try_recv().is_ok() {}
    self.backbone_rx.lock().expect("backbone lock").replace(backbone_rx);

    // Create fresh session channel
    let (session_tx, session_rx) = mpsc::channel(CHANNEL_BUFFER);
    self.session_tx = Some(session_tx);
    self.session_rx = Some(session_rx);

    // Notify remote side
    let _ = self.backbone_tx.send(ChannelEnvelope::Connected).await;

    // Transition through Connecting to match SshTransport's status lifecycle
    let mut status = self.status.lock().expect("status lock");
    *status = PeerConnectionStatus::Connecting;
    *status = PeerConnectionStatus::Connected;

    Ok(())
}
```

**`PeerTransport` impl — `disconnect()`:**
```rust
async fn disconnect(&mut self) -> Result<(), String> {
    // Guard against double-disconnect: no-op if no active session
    if self.cancel_tx.is_none() && self.task_handle.is_none() && self.session_tx.is_none() {
        return Ok(());
    }

    // Notify remote side (best-effort)
    let _ = self.backbone_tx.send(ChannelEnvelope::Disconnected).await;

    // Signal forwarding task to exit (if running)
    self.cancel_tx.take();

    // Await task completion — ensures backbone_rx is returned
    if let Some(handle) = self.task_handle.take() {
        let _ = handle.await;
    }

    // Drop session channels
    self.session_tx.take();
    self.session_rx.take();

    // Update status
    *self.status.lock().expect("status lock") = PeerConnectionStatus::Disconnected;

    Ok(())
}
```

**`PeerTransport` impl — `status()`:**
```rust
fn status(&self) -> PeerConnectionStatus {
    self.status.lock().expect("status lock").clone()
}
```

**`PeerTransport` impl — `subscribe()`:**

Takes `session_rx` AND spawns the forwarding task. The task takes `backbone_rx` and `session_tx`.

```rust
async fn subscribe(&mut self) -> Result<mpsc::Receiver<PeerWireMessage>, String> {
    {
        let status = self.status.lock().expect("status lock");
        if *status != PeerConnectionStatus::Connected {
            return Err(format!("cannot subscribe: status is {:?}", *status));
        }
    }

    let session_rx = self.session_rx.take()
        .ok_or_else(|| "already subscribed (receiver already taken)".to_string())?;

    // Take backbone_rx and session_tx for the forwarding task
    let backbone_rx = self.backbone_rx.lock().expect("backbone lock")
        .take()
        .ok_or("cannot subscribe: backbone receiver unavailable")?;
    let session_tx = self.session_tx.take()
        .ok_or("cannot subscribe: session sender unavailable")?;

    // Create cancellation channel
    let (cancel_tx, cancel_rx) = oneshot::channel();
    self.cancel_tx = Some(cancel_tx);

    // Spawn forwarding task
    let status = Arc::clone(&self.status);
    let backbone_rx_slot = Arc::clone(&self.backbone_rx);
    self.task_handle = Some(tokio::spawn(forwarding_task(
        backbone_rx, session_tx, cancel_rx, status, backbone_rx_slot,
    )));

    Ok(session_rx)
}
```

**`PeerTransport` impl — `sender()`:**
```rust
fn sender(&self) -> Option<Arc<dyn PeerSender>> {
    let status = self.status.lock().expect("status lock");
    if *status != PeerConnectionStatus::Connected {
        return None;
    }
    Some(Arc::new(ChannelSender {
        tx: tokio::sync::Mutex::new(Some(self.backbone_tx.clone())),
    }) as Arc<dyn PeerSender>)
}
```

Note: `sender()` no longer needs `outbound_tx` — it wraps `backbone_tx` directly, sending `Packet` envelopes. Previously returned senders remain functional after disconnect (matching TCP semantics — they can still enqueue to the backbone).

**`ChannelSender`:**
```rust
pub struct ChannelSender {
    tx: tokio::sync::Mutex<Option<mpsc::Sender<ChannelEnvelope>>>,
}

#[async_trait]
impl PeerSender for ChannelSender {
    async fn send(&self, msg: PeerWireMessage) -> Result<(), String> {
        let tx = self.tx.lock().await;
        let tx = tx.as_ref().ok_or_else(|| "channel sender retired".to_string())?;
        tx.send(ChannelEnvelope::Packet(msg)).await.map_err(|_| "channel closed".to_string())
    }

    async fn retire(&self, reason: GoodbyeReason) -> Result<(), String> {
        let tx = self.tx.lock().await.take();
        if let Some(tx) = tx {
            tx.send(ChannelEnvelope::Packet(PeerWireMessage::Goodbye { reason }))
                .await
                .map_err(|_| "channel closed".to_string())?;
        }
        Ok(())
    }
}
```

**Forwarding task** (free async function):
```rust
async fn forwarding_task(
    mut backbone_rx: mpsc::Receiver<ChannelEnvelope>,
    session_tx: mpsc::Sender<PeerWireMessage>,
    mut cancel_rx: oneshot::Receiver<()>,
    status: Arc<std::sync::Mutex<PeerConnectionStatus>>,
    backbone_rx_slot: Arc<std::sync::Mutex<Option<mpsc::Receiver<ChannelEnvelope>>>>,
) {
    loop {
        tokio::select! {
            envelope = backbone_rx.recv() => {
                match envelope {
                    Some(ChannelEnvelope::Packet(msg)) => {
                        // Forward to session channel; ignore error if subscriber dropped
                        let _ = session_tx.send(msg).await;
                    }
                    Some(ChannelEnvelope::Disconnected) => {
                        // Remote disconnected — update status BEFORE closing session,
                        // so subscribers see Disconnected as soon as recv() returns None
                        *status.lock().expect("status lock") = PeerConnectionStatus::Disconnected;
                        drop(session_tx);
                        backbone_rx_slot.lock().expect("backbone lock").replace(backbone_rx);
                        return;
                    }
                    Some(ChannelEnvelope::Connected) => {
                        // Remote reconnected — no-op
                    }
                    None => {
                        // Backbone closed (peer transport dropped) — return backbone, exit
                        // Don't update status; the owning side will discover the closure
                        backbone_rx_slot.lock().expect("backbone lock").replace(backbone_rx);
                        return;
                    }
                }
            }
            _ = &mut cancel_rx => {
                // Local disconnect — return backbone receiver and exit
                backbone_rx_slot.lock().expect("backbone lock").replace(backbone_rx);
                return;
            }
        }
    }
}
```

**`channel_transport_pair()` and accessors:**
```rust
impl ChannelTransport {
    pub fn local_name(&self) -> &HostName {
        &self.local_name
    }

    pub fn remote_name(&self) -> &HostName {
        &self.remote_name
    }
}

/// Create a paired set of in-process transports. A's outbound backbone is B's
/// inbound backbone and vice versa. Both start in `Disconnected` state.
pub fn channel_transport_pair(local_name: HostName, remote_name: HostName) -> (ChannelTransport, ChannelTransport) {
    let (a_to_b_tx, a_to_b_rx) = mpsc::channel(CHANNEL_BUFFER);
    let (b_to_a_tx, b_to_a_rx) = mpsc::channel(CHANNEL_BUFFER);

    let transport_a = ChannelTransport {
        local_name: local_name.clone(),
        remote_name: remote_name.clone(),
        status: Arc::new(std::sync::Mutex::new(PeerConnectionStatus::Disconnected)),
        backbone_tx: a_to_b_tx,
        backbone_rx: Arc::new(std::sync::Mutex::new(Some(b_to_a_rx))),
        session_tx: None,
        session_rx: None,
        cancel_tx: None,
        task_handle: None,
    };

    let transport_b = ChannelTransport {
        local_name: remote_name,
        remote_name: local_name,
        status: Arc::new(std::sync::Mutex::new(PeerConnectionStatus::Disconnected)),
        backbone_tx: b_to_a_tx,
        backbone_rx: Arc::new(std::sync::Mutex::new(Some(a_to_b_rx))),
        session_tx: None,
        session_rx: None,
        cancel_tx: None,
        task_handle: None,
    };

    (transport_a, transport_b)
}
```

- [ ] **Step 2: Update the `reconnect_after_disconnect` test**

Change `reconnect_after_disconnect_fails` to `reconnect_after_disconnect_succeeds`:

```rust
    #[tokio::test]
    async fn reconnect_after_disconnect_succeeds() {
        let (mut a, _b) = channel_transport_pair(HostName::new("alpha"), HostName::new("beta"));
        a.connect().await.expect("connect should succeed");
        a.disconnect().await.expect("disconnect should succeed");
        a.connect().await.expect("reconnect should succeed");
        assert_eq!(a.status(), PeerConnectionStatus::Connected);
    }
```

All other 13 existing tests pass unchanged because:
- Tests that drop `_b` without subscribing: no forwarding task is running, so the backbone closure doesn't trigger any status change. `connect()` still works because backbone_rx is still in the arc (never taken by a forwarding task).
- Tests that connect both sides and subscribe: the forwarding task runs and handles envelopes. `disconnect()` sends `Disconnected`, the remote task receives it and closes the session. Same observable behavior as before.

- [ ] **Step 3: Run tests to verify all pass**

Run: `cargo test -p flotilla-daemon channel_transport::tests`
Expected: All 14 tests PASS

- [ ] **Step 4: Run the integration tests**

Run: `cargo test -p flotilla-daemon peer::channel_tests`
Expected: All 7 tests PASS (TestNetwork calls subscribe via connect_all, so forwarding tasks run normally)

- [ ] **Step 5: Run full test suite and lint**

Run: `cargo test -p flotilla-daemon --locked && cargo clippy -p flotilla-daemon --all-targets --locked -- -D warnings`
Expected: All PASS, no warnings

- [ ] **Step 6: Format and commit**

Run: `cargo +nightly fmt`

```bash
git add crates/flotilla-daemon/src/peer/channel_transport.rs
git commit -m "refactor: rewrite ChannelTransport with backbone+session reconnection support"
```

### Task 2: Reconnection Unit Tests

**Files:**
- Modify: `crates/flotilla-daemon/src/peer/channel_transport.rs` (test module)

- [ ] **Step 1: Write reconnection tests**

Add these tests to the `#[cfg(test)] mod tests` block:

```rust
    #[tokio::test]
    async fn reconnect_sends_and_receives() {
        let (mut a, mut b) = channel_transport_pair(HostName::new("alpha"), HostName::new("beta"));

        // First session
        a.connect().await.expect("connect A");
        b.connect().await.expect("connect B");
        let sender_a1 = a.sender().expect("sender A");
        let mut rx_b1 = b.subscribe().await.expect("subscribe B");
        sender_a1.send(test_snapshot_msg("alpha", 1)).await.expect("send");
        let msg = rx_b1.recv().await.expect("recv");
        assert!(matches!(msg, PeerWireMessage::Data(PeerDataMessage { kind: PeerDataKind::Snapshot { seq: 1, .. }, .. })));

        // Disconnect both sides
        a.disconnect().await.expect("disconnect A");
        b.disconnect().await.expect("disconnect B");

        // Second session — reconnect and verify messaging works
        a.connect().await.expect("reconnect A");
        b.connect().await.expect("reconnect B");
        let sender_a2 = a.sender().expect("sender A after reconnect");
        let mut rx_b2 = b.subscribe().await.expect("subscribe B after reconnect");
        sender_a2.send(test_snapshot_msg("alpha", 2)).await.expect("send after reconnect");
        let msg = rx_b2.recv().await.expect("recv after reconnect");
        assert!(matches!(msg, PeerWireMessage::Data(PeerDataMessage { kind: PeerDataKind::Snapshot { seq: 2, .. }, .. })));
    }

    #[tokio::test]
    async fn remote_disconnect_closes_local_receiver() {
        let (mut a, mut b) = channel_transport_pair(HostName::new("alpha"), HostName::new("beta"));
        a.connect().await.expect("connect A");
        b.connect().await.expect("connect B");

        let _rx_a = a.subscribe().await.expect("subscribe A");
        let mut rx_b = b.subscribe().await.expect("subscribe B");

        // A disconnects — B's forwarding task should see Disconnected and close the session
        a.disconnect().await.expect("disconnect A");

        // B's receiver should yield None
        let msg = rx_b.recv().await;
        assert!(msg.is_none(), "B's receiver should close after A disconnects");
    }

    #[tokio::test]
    async fn remote_disconnect_transitions_status() {
        let (mut a, mut b) = channel_transport_pair(HostName::new("alpha"), HostName::new("beta"));
        a.connect().await.expect("connect A");
        b.connect().await.expect("connect B");

        let _rx_a = a.subscribe().await.expect("subscribe A");
        let mut rx_b = b.subscribe().await.expect("subscribe B");

        // A disconnects
        a.disconnect().await.expect("disconnect A");

        // Drain B's receiver to ensure the forwarding task has processed the Disconnected envelope
        let _ = rx_b.recv().await;

        // B's status should now be Disconnected
        assert_eq!(b.status(), PeerConnectionStatus::Disconnected,
            "B should transition to Disconnected after A disconnects");
    }

    #[tokio::test]
    async fn reconnect_after_remote_disconnect() {
        let (mut a, mut b) = channel_transport_pair(HostName::new("alpha"), HostName::new("beta"));
        a.connect().await.expect("connect A");
        b.connect().await.expect("connect B");

        let _rx_a = a.subscribe().await.expect("subscribe A");
        let mut rx_b = b.subscribe().await.expect("subscribe B");

        // A disconnects — B detects
        a.disconnect().await.expect("disconnect A");
        let _ = rx_b.recv().await; // drain to trigger status transition
        assert_eq!(b.status(), PeerConnectionStatus::Disconnected);

        // Both sides reconnect
        a.connect().await.expect("reconnect A");
        b.connect().await.expect("reconnect B");

        // Bidirectional messaging works in the new session
        let sender_a = a.sender().expect("sender A");
        let sender_b = b.sender().expect("sender B");
        let mut rx_a = a.subscribe().await.expect("subscribe A");
        let mut rx_b = b.subscribe().await.expect("subscribe B");

        sender_a.send(test_snapshot_msg("alpha", 10)).await.expect("send A→B");
        sender_b.send(test_snapshot_msg("beta", 20)).await.expect("send B→A");

        let msg_at_b = rx_b.recv().await.expect("B recv");
        assert!(matches!(msg_at_b, PeerWireMessage::Data(PeerDataMessage { kind: PeerDataKind::Snapshot { seq: 10, .. }, .. })));

        let msg_at_a = rx_a.recv().await.expect("A recv");
        assert!(matches!(msg_at_a, PeerWireMessage::Data(PeerDataMessage { kind: PeerDataKind::Snapshot { seq: 20, .. }, .. })));
    }

    #[tokio::test]
    async fn multiple_reconnect_cycles() {
        let (mut a, mut b) = channel_transport_pair(HostName::new("alpha"), HostName::new("beta"));

        for cycle in 0..3u64 {
            a.connect().await.unwrap_or_else(|e| panic!("connect A cycle {cycle}: {e}"));
            b.connect().await.unwrap_or_else(|e| panic!("connect B cycle {cycle}: {e}"));

            let sender = a.sender().expect("sender A");
            let mut rx = b.subscribe().await.expect("subscribe B");

            let seq = cycle * 10 + 1;
            sender.send(test_snapshot_msg("alpha", seq)).await.expect("send");
            let msg = rx.recv().await.expect("recv");
            assert!(matches!(msg, PeerWireMessage::Data(PeerDataMessage { kind: PeerDataKind::Snapshot { seq: s, .. }, .. }) if s == seq));

            a.disconnect().await.unwrap_or_else(|e| panic!("disconnect A cycle {cycle}: {e}"));
            b.disconnect().await.unwrap_or_else(|e| panic!("disconnect B cycle {cycle}: {e}"));
        }
    }
```

Note: The `remote_disconnect_closes_local_receiver`, `remote_disconnect_transitions_status`, and `reconnect_after_remote_disconnect` tests call `subscribe()` on BOTH sides. This is required because `subscribe()` spawns the forwarding task — without subscribing on A's side, A's `disconnect()` wouldn't send the `Disconnected` envelope through a task that B's task can observe. Actually, `disconnect()` sends `Disconnected` directly on the backbone (not through the forwarding task), so B's forwarding task will see it regardless. BUT: A needs to have subscribed so that A's forwarding task is running and can be properly cancelled by `disconnect()`. The `_rx_a` binding keeps A's subscriber alive.

- [ ] **Step 2: Run tests to verify all pass**

Run: `cargo test -p flotilla-daemon channel_transport::tests`
Expected: All 19 tests PASS (14 updated originals + 5 new)

- [ ] **Step 3: Run full suite and lint**

Run: `cargo test -p flotilla-daemon --locked && cargo clippy -p flotilla-daemon --all-targets --locked -- -D warnings`
Expected: All PASS, no warnings

- [ ] **Step 4: Format and commit**

Run: `cargo +nightly fmt`

```bash
git add crates/flotilla-daemon/src/peer/channel_transport.rs
git commit -m "test: reconnection unit tests for ChannelTransport"
```

---

## Implementation Notes

### Why lazy task spawn in `subscribe()` not `connect()`

If the forwarding task were spawned in `connect()`, dropping a transport without subscribing (common in single-side tests like `connect_transitions_to_connected`) would cause the task to receive `None` from `backbone_rx.recv()` when the other side's `backbone_tx` is dropped. The task would then set status to `Disconnected`, racing with test assertions that expect `Connected`.

By spawning in `subscribe()`, the task only runs when someone is actively consuming messages. Tests that never subscribe never have a running task and never observe backbone closure. This matches the old implementation where `inbound_rx` was passive.

The `connect_all()` path in `PeerManager` calls `connect()` → `sender()` → `subscribe()` in sequence, so the task is spawned at the right time for production use.

### Why `std::sync::Mutex` not `tokio::sync::Mutex`

Both `status` and `backbone_rx` use `std::sync::Mutex`. The forwarding task must return `backbone_rx` on all exit paths. `std::sync::Mutex::lock()` is safe in non-async contexts and completes in bounded time. The locks are held only briefly for status reads/writes and receiver take/put — never across `.await` points.

### Stale envelope draining

`connect()` drains the backbone receiver with `try_recv()` before storing it back. This discards leftover envelopes from previous sessions (e.g., `Disconnected` + `Connected` from the remote side reconnecting while the local side was down).

### `disconnect_closes_receiver` test — behavioral change

In the old implementation, `disconnect()` directly dropped `outbound_tx`, closing the other side's `inbound_rx`. In the new implementation, `disconnect()` sends a `Disconnected` envelope on the backbone, and the remote's forwarding task receives it and drops `session_tx`, closing the subscriber's `session_rx`. The observable behavior is the same (B's `rx.recv()` returns `None`) but the mechanism is different. The test passes without changes.

### Backbone closure handling in forwarding task

When `backbone_rx.recv()` returns `None` (the peer transport was dropped, closing `backbone_tx`), the task does NOT set status to `Disconnected`. It just returns the backbone receiver and exits. This prevents races in tests where `_b` is dropped — the owning transport's status remains `Connected` until explicitly disconnected. The subscriber will get `None` from the session channel (because `session_tx` is dropped when the task exits), but this only matters if someone is actively subscribing.

### Double-disconnect safety

`disconnect()` is a no-op if `cancel_tx`, `task_handle`, and `session_tx` are all `None`. This prevents sending spurious `Disconnected` envelopes that could incorrectly terminate a remote session that reconnected.
