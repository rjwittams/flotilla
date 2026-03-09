# Record/Replay Test Harness — Design

## Problem

Provider tests need to verify interactions with external tools (git, wt, gh, tmux, zellij, claude API) without running them. The current MockRunner returns canned responses but doesn't capture what commands were issued, making tests weak. Hand-writing mock setups is tedious and fragile.

## Design

### Core Concept

A **ReplaySession** is a shared interaction log backed by a YAML file. Multiple adapters (CommandRunner, GhApi, HTTP) share one session. Each adapter reads/writes entries tagged with its channel.

Tests are still normal Rust code — they construct providers, call methods, assert on results. The session just handles I/O canning.

Two modes:
- **Replay** (default): serve canned responses from YAML, panic if the interaction diverges
- **Record** (`RECORD=1`): call through to real implementations, write YAML at end

### YAML Format

```yaml
# fixtures/git_branches.yaml
interactions:
  - channel: command
    cmd: git
    args: ["branch", "--list", "--format=%(refname:short)"]
    cwd: "{repo}"
    stdout: "main\nfeature-1\nfeature-2\n"
    exit_code: 0

  - channel: command
    cmd: git
    args: ["rev-list", "--count", "--left-right", "feature-1...main"]
    cwd: "{repo}"
    stdout: "2\t3\n"
    exit_code: 0

  - channel: gh_api
    method: GET
    endpoint: "/repos/owner/repo/pulls?state=all"
    status: 200
    body: |
      [{"number": 1, "title": "Fix bug", "state": "open"}]

  - channel: http
    method: GET
    url: "https://api.anthropic.com/v1/sessions"
    status: 200
    body: |
      {"data": [{"uuid": "s1", "name": "test"}]}
```

### Masking

Paths and timestamps are replaced with placeholders during recording:
- Repo root paths → `{repo}`
- Home directory → `{home}`
- Timestamps → `{timestamp}`
- OAuth tokens → `{token}`

A `Masks` struct holds the substitutions, applied on record (concrete → placeholder) and replay (placeholder → concrete).

### Components

```
ReplaySession (Arc<Mutex<inner>>)
├── interactions: Vec<Interaction>  (the YAML data)
├── cursor: usize                   (shared read position)
├── masks: Masks                    (placeholder substitutions)
└── mode: Mode                      (Replay | Record)

ReplayRunner: CommandRunner          ── reads channel: "command"
ReplayGhApi: GhApi                   ── reads channel: "gh_api"
ReplayHttpClient: HttpClient trait   ── reads channel: "http"  (future)
```

### Interaction Enum

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel")]
enum Interaction {
    #[serde(rename = "command")]
    Command {
        cmd: String,
        args: Vec<String>,
        cwd: String,
        stdout: Option<String>,
        stderr: Option<String>,
        exit_code: i32,
    },
    #[serde(rename = "gh_api")]
    GhApi {
        method: String,
        endpoint: String,
        status: u16,
        body: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
    #[serde(rename = "http")]
    Http {
        method: String,
        url: String,
        status: u16,
        #[serde(default)]
        request_body: Option<String>,
        body: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}
```

### Matching in Replay Mode

When `ReplayRunner::run(cmd, args, cwd)` is called:
1. Advance cursor to next `Command` entry
2. Assert `cmd` matches (panic with diff if not)
3. Assert `args` match (panic with diff if not)
4. Return `stdout`/`stderr` based on `exit_code`

This is strict ordered matching — the test fails if the interaction sequence diverges. This is intentional: the recording IS the specification of what commands should be issued.

### Record Mode

When `RECORD=1` is set:
1. `ReplayRunner` wraps a real `ProcessCommandRunner`
2. Each call is forwarded to the real runner
3. The result is captured as an `Interaction` and appended to the session log
4. On drop, the session writes the YAML file

Tests run against real tools in record mode, producing the fixture files. This requires the tools to be installed — record mode is for the developer's machine, replay mode is for CI.

### Test Usage

```rust
#[tokio::test]
async fn git_branches_parses_ahead_behind() {
    let session = ReplaySession::from_file("fixtures/git_branches.yaml");
    let runner = session.command_runner();
    let git = GitVcs::new(Arc::new(runner), &repo_path);

    let branches = git.branches().await.unwrap();

    assert_eq!(branches.len(), 3);
    assert_eq!(branches[0].name, "main");
}
```

Multi-channel:
```rust
#[tokio::test]
async fn github_pr_list() {
    let session = ReplaySession::from_file("fixtures/github_prs.yaml");
    let runner = session.command_runner();
    let gh_api = session.gh_api();
    let provider = GitHubCodeReview::new(Arc::new(gh_api), "owner/repo");

    let prs = provider.list().await.unwrap();
    assert_eq!(prs.len(), 1);
}
```

### File Location

Fixture files live alongside tests:
- `crates/flotilla-core/src/providers/vcs/fixtures/git_branches.yaml`
- `crates/flotilla-core/src/providers/workspace/fixtures/tmux_list.yaml`
- etc.

### What This Doesn't Do

- No fuzzy matching or partial arg assertions — strict sequence matching
- No mock expectations API — tests assert on domain results, not mock calls
- No parallel test isolation issues — each test gets its own session file
- No HTTP client trait needed initially — #92 can use `wiremock` or add the HTTP channel later

### Scope for Initial Implementation

1. **Framework**: `Interaction` enum, `ReplaySession`, `Masks`, YAML serde
2. **CommandRunner adapter**: `ReplayRunner` (replay mode only first)
3. **GhApi adapter**: `ReplayGhApi` (replay mode only)
4. **Apply to git.rs tests** — prove the pattern works
5. **Record mode** — add the passthrough recording wrapper
6. **Apply to remaining providers** — wt.rs, tmux.rs, zellij.rs, github providers

HTTP channel (#92) deferred — claude.rs can start with just CommandRunner for the `security` CLI parts.
