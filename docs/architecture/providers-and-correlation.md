# Providers And Correlation

The main architectural decision in Flotilla is to model integrations as
providers that emit normalized data, then correlate that data into work items.
That avoids scattering branch-matching and tool-specific glue throughout the
UI.

## Provider Families

The registry currently supports:

- `Vcs`
- `CheckoutManager`
- `CodeReview`
- `IssueTracker`
- `CodingAgent`
- `AiUtility`
- one selected `WorkspaceManager`

Only some families have multiple implementations today, but the registry shape
assumes more backends will continue to arrive.

## Detection Model

Provider detection is environment-first:

- git repos are detected from `.git`
- checkout strategy is chosen from repo config plus available tools
- remote host is inferred from git remote URL
- GitHub providers are enabled when the repo remote and `gh` CLI line up
- Claude providers are enabled from CLI availability
- workspace-manager selection prefers the terminal environment the process is
  running inside

The same binary can therefore behave differently per repo and per shell session
without a large explicit config matrix.

## Normalized Provider Data

Providers write into a shared `ProviderData` structure:

- `checkouts`
- `change_requests`
- `issues`
- `sessions`
- `workspaces`
- `branches`

These collections stay separate until correlation. Providers should report facts
they own directly, not pre-merge them into a final work item.

## Correlation Keys

Providers attach `CorrelationKey` and `AssociationKey` values so separate
records can be merged later.

Important examples:

- branch names correlate checkouts, change requests, sessions, and workspaces
- checkout paths correlate workspace directories back to local checkouts
- issue references associate checkouts or change requests with issues

The correlation engine uses union-find to merge items transitively:

- if A shares a key with B, and B shares a different key with C, all three land
  in one group
- merges that would create two singleton kinds in one group are refused

Today, checkouts and change requests are singleton kinds. That rule prevents
obviously invalid rows, but it also exposes where upstream tools are giving
Flotilla an imperfect model.

## Materializing Work Items

After correlation, core chooses an anchor item for each group:

- prefer checkout
- else change request
- else coding session

Issues and remote branches remain standalone if they are not absorbed into a
group. The resulting work item is flattened into protocol data with:

- identity and kind
- branch
- description
- optional linked checkout / change request / session
- related issue keys
- related workspace refs

This materialized form is what clients render.

## Issue Cache Overlay

Issues are special compared with other provider data:

- the refresh loop does not fetch the entire issue set every cycle
- the daemon owns an `IssueCache` per repo
- cached issues are injected into provider data just before correlation and
  snapshot construction
- linked issues can be pinned so correlated rows keep their issue context even
  when the general issue list is paginated

This makes issues a daemon-owned overlay on top of refresh snapshots rather than
just another provider collection.

## Extension Rule

New integrations should fit an existing provider family where possible, emit
normalized provider data, and participate in correlation. They should not add
parallel UI-specific data paths just because the first implementation is
tool-specific.
