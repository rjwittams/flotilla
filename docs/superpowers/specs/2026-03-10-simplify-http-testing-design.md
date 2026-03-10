# Simplify HTTP testing in the replay framework

## Problem

PR #180 added test coverage for `ClaudeCodingAgent` but introduced unnecessary
abstraction: a `ClaudeHttp` trait with `ReqwestClaudeHttp` (production) and
`ReplayClaudeHttp` (test) wrappers, plus a standalone `ReplayHttp` struct and
custom `HttpResponse` types in both `replay.rs` and `claude.rs`. The claude.rs
tests also construct interactions in-memory instead of using fixture YAML files,
breaking the pattern every other provider follows.

## Design

### New `HttpClient` trait

A single trait in `providers/`, alongside `CommandRunner` and `GhApi`:

```rust
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, req: reqwest::Request) -> Result<http::Response<Bytes>, String>;
}
```

- Input: `reqwest::Request` -- callers use the ergonomic reqwest builder API.
- Output: `http::Response<Bytes>` -- the standard Rust HTTP response type that
  reqwest is built on. Trivially constructable in tests, and `TryFrom`
  conversions exist in both directions if we ever need `reqwest::Response`.

### Production implementation

Thin wrapper around `reqwest::Client::execute()`, extracting status/headers/body
into `http::Response<Bytes>`.

### Replay implementation

`ReplayHttpClient` in `replay.rs` implements `HttpClient`. Reads
`Interaction::Http` entries from the replay session and constructs
`http::Response<Bytes>` from fixture data. Lives in the replay module alongside
`ReplayRunner` and `ReplayGhApi`.

### `ClaudeCodingAgent` changes

Takes `Arc<dyn HttpClient>` alongside `Arc<dyn CommandRunner>`, matching the
dependency injection pattern used by every other provider. Tests inject the
replay implementation with fixture YAML files in `coding_agent/fixtures/`.

### What to remove

- `ClaudeHttp` trait, `ReqwestClaudeHttp`, `ReplayClaudeHttp` (claude.rs)
- `HttpResponse` structs in both `replay.rs` and `claude.rs`
- `ReplayHttp` struct (replaced by `ReplayHttpClient` implementing `HttpClient`)
- The `_with_http` / `_with_base` indirection chain -- replaced by injected dep
- In-memory interaction construction in claude.rs tests

### What to keep

- `Interaction::Http` variant in the replay framework (now used by fixture files)
- `Interaction::GhApi` variant (higher-level abstraction over `gh` CLI calls,
  used by existing github fixture files)
- All test logic and assertions -- just rewired to use fixtures and the new trait

### Fixture file convention

Tests should follow the pattern established by existing providers:

```
crates/flotilla-core/src/providers/
  coding_agent/
    fixtures/
      claude_sessions.yaml       # list sessions (200 response)
      claude_auth_retry.yaml     # 401 then retry with fresh token
      claude_auth_failure.yaml   # repeated auth failures -> empty list
      claude_archive.yaml        # archive session PATCH calls
    claude.rs
  code_review/
    fixtures/
      github_prs.yaml
      github_merged.yaml
    github.rs
  ...
```

Each fixture is a YAML file with `Interaction` entries. Tests load via
`ReplaySession::from_file()`. Record mode (`RECORD=1`) writes real responses
to the fixture path for bootstrapping.

## Agent guidelines for record/replay

When writing tests for providers that call external services:

1. **Use fixture YAML files**, not in-memory interaction construction.
2. **One fixture per test scenario** -- name it after what it tests.
3. **Place fixtures in `<provider>/fixtures/`** next to the provider source.
4. **Use masks** for non-deterministic values (paths, tokens, timestamps).
5. **Inject dependencies** via `Arc<dyn Trait>` -- use replay implementations
   from `replay.rs` for `CommandRunner`, `GhApi`, and `HttpClient`.
6. **Call `session.assert_complete()`** (or `session.finish()`) to verify all
   fixture interactions were consumed.
7. **Bootstrap with `RECORD=1`** when creating new fixtures against real APIs,
   then review and sanitize the recorded YAML.
