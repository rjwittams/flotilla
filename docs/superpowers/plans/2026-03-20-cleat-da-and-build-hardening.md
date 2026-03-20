# cleat-4 plan: detached DA handling and Ghostty build hardening

## Goal

Close the next two follow-up priorities after `capture` and `send-keys`:

1. Handle device-attribute (DA) queries while no foreground client is attached.
2. Make the optional `ghostty-vt` build path less ad hoc for contributors and CI.

The emphasis for this branch is correctness first, not breadth. We should make detached sessions behave predictably for shells that probe terminal capabilities, and make the Ghostty feature path explicit and repeatable without forcing Zig/Ghostty on default builds.

## Scope

In scope:

- Detect DA queries in PTY output while detached.
- Synthesize bounded replies for the supported DA queries when no foreground client is attached.
- Keep the behavior daemon-local and replay-safe.
- Add explicit local build expectations for `ghostty-vt`.
- Add a minimal CI/build contract for the optional feature.

Out of scope:

- General escape-sequence answering beyond DA1/DA2.
- Full terminal capability emulation.
- Capability downconversion.
- Rich remote install automation for Ghostty/Zig beyond what is needed to make the feature path explicit.
- `view` or further control-plane features.

## Design decisions

### 1. Detached DA handling should be narrow and explicit

We should only answer the specific queries we have evidence for:

- `ESC [ c` (DA1)
- `ESC [ > c` (DA2)

Anything else should continue to flow normally.

Reasoning:

- the immediate problem from the research is shells hanging or warning when detached
- broad terminal emulation is a trap
- a narrow allowlist is easier to reason about and test

### 2. DA replies are only synthesized when no foreground client is attached

When a foreground client is attached:

- the real outer terminal should answer
- `cleat` should not race it or spoof replies

When detached:

- the daemon must reply on behalf of the absent terminal

That keeps the contract simple and avoids dueling replies.

### 3. Synthesis happens on the PTY-output path

The daemon already sees PTY output bytes before broadcasting and before/while feeding VT state.

The right seam is:

- inspect PTY output chunks for DA queries
- when detached, write the synthetic response bytes back into the PTY input side
- continue handling normal PTY output as before

This avoids inventing a second parser path around client IO.

### 4. Keep Ghostty feature hardening opt-in

Default builds must remain:

- Rust-only
- Zig-free
- Ghostty-free

The hardening goal is:

- predictable feature-on local builds
- predictable feature-on CI job(s)

Not:

- making `ghostty-vt` mandatory

## Work plan

### Task 1: specify detached DA behavior

Add a small internal spec in code/tests for:

- which DA queries are recognized
- the exact synthetic bytes returned
- when synthesis is enabled or disabled

Deliverables:

- small helper module for DA detection/replies
- unit tests for query recognition and reply generation

### Task 2: wire detached DA synthesis into the daemon loop

Integrate the helper into the PTY-output handling path in `crates/cleat/src/session.rs`.

Required behavior:

- when detached and PTY output contains DA1/DA2 query bytes, synthesize the reply into the PTY input side
- when attached, do nothing special
- normal output handling must continue unchanged

Deliverables:

- daemon integration
- focused lifecycle/integration test proving detached DA queries are answered

### Task 3: verify no foreground/replay regressions

Add regression coverage for:

- detached DA handling does not break normal output capture/replay
- attached sessions do not get synthetic replies from the daemon

Deliverables:

- focused lifecycle tests

### Task 4: Ghostty local build contract

Make the optional Ghostty path explicit in-repo.

Minimum work:

- document the expected local Ghostty install prefix
- document the supported Zig/Ghostty assumptions for this branch
- tighten `build.rs` messages if needed so feature-on failures clearly explain what is missing

Deliverables:

- docs update
- clearer feature-on diagnostics if needed

### Task 5: Ghostty CI hardening

Add a dedicated optional-feature CI path rather than changing default CI.

Minimum goal:

- one CI job that enables `ghostty-vt`
- build and test `cleat` with the feature on

This job may rely on:

- pinned Zig install in CI
- a pinned Ghostty checkout/build step or artifact path

The key requirement is that the contract is written down and reproducible.

Deliverables:

- CI workflow update
- feature-on verification command(s)

## Verification plan

Minimum branch verification before PR:

- `cargo +nightly-2026-03-12 fmt --check`
- `cargo clippy -p cleat --all-targets --locked -- -D warnings`
- `cargo test -p cleat --locked`

If Ghostty/CI hardening lands on this branch too:

- `cargo clippy -p cleat --all-targets --locked --features ghostty-vt -- -D warnings`
- `cargo test -p cleat --locked --features ghostty-vt`

## Risks

### Risk 1: DA query detection is too broad

Mitigation:

- restrict to exact DA1/DA2 patterns
- add precise unit tests

### Risk 2: synthetic replies race with real terminal replies

Mitigation:

- only synthesize when no foreground client is attached

### Risk 3: Ghostty CI hardening balloons in scope

Mitigation:

- keep default CI untouched
- add one dedicated feature-on path only

## Recommended execution order

1. DA helper and tests
2. daemon integration for detached DA replies
3. detached/attached regression tests
4. Ghostty local build-contract cleanup
5. Ghostty CI hardening

