# Project Website Design

## Goal

Create a project website for flotilla at `flotilla.work` — a landing page with accurate product content, a blog for thought-leadership, and CI artifact hosting (coverage reports, badges). Hosted on GitHub Pages from a separate private repo.

## Repository

- **Repo**: `flotilla-org/site` (private)
- **Framework**: Astro (`output: 'static'`) with Tailwind CSS
- **Hosting**: GitHub Pages via `actions/deploy-pages@v4`
- **Domain**: `flotilla.work`, CNAME pointing to `flotilla-org.github.io`

## Site Structure

```
flotilla.work/                → Landing page
flotilla.work/blog/           → Blog index
flotilla.work/blog/<slug>/    → Individual blog posts
flotilla.work/i/coverage/     → Coverage report (from main repo)
flotilla.work/i/badges/       → Badge JSON endpoints (shields.io compatible)
```

The `/i/` prefix namespaces internal/CI artifacts, keeping the top-level URL space clean for future services (Cloudflare or AWS-style routing).

## Cross-Repo Coverage Stitching

The nightly workflow in `flotilla-org/flotilla` already produces `coverage-site` and `dylint-badge` artifacts. To unify them under `flotilla.work`:

### Authentication

Cross-repo operations require a GitHub App installation token or a PAT with appropriate scopes, stored as an org-level secret (e.g., `SITE_DEPLOY_TOKEN`):

- **`repository_dispatch`** from flotilla → site requires `contents: write` on the target repo.
- **Cross-repo artifact download** from site → flotilla requires `actions: read` on the source repo.

A single fine-grained PAT scoped to both repos with `contents: write` + `actions: read` covers both. A GitHub App is cleaner long-term but a PAT is simpler to start.

### Flow

1. **Main repo nightly job** adds a final step: `repository_dispatch` to `flotilla-org/site` with event type `coverage-updated`, payload `{"run_id": "${{ github.run_id }}"}`. Uses `SITE_DEPLOY_TOKEN` for auth. The existing `deploy` job in `coverage.yml` is removed — all Pages hosting moves to the site repo.
2. **Site repo workflow** triggers on `repository_dispatch` (plus its own `push` trigger for content changes).
3. Site workflow uses the GitHub REST API (`GET /repos/flotilla-org/flotilla/actions/runs/{run_id}/artifacts`) with `SITE_DEPLOY_TOKEN` to download the coverage and badge artifact zips, then extracts them.
4. Coverage HTML goes to `public/i/coverage/`. The coverage `badge.json` (from inside the coverage artifact) and the dylint `dylint.json` (from the dylint-badge artifact) both go to `public/i/badges/`.

The main repo's existing `deploy` job in `coverage.yml` is removed as part of this work.

## Landing Page

### Content Sections

**Hero**: Flotilla is a TUI dashboard that unifies your dev workflow — git worktrees, PRs, issues, coding agent sessions, and terminal workspaces — into one correlated view.

**Feature blocks** (4 sections):

1. **Correlated work items** — A union-find engine links branches, PRs, issues, and agent sessions into unified work items. No manual bookkeeping across tools.
2. **Multi-host mesh** — Peer-to-peer daemon over SSH with vector clocks and routed commands. Manage work across machines from one TUI.
3. **Workspace templates** — `.flotilla/workspace.yaml` drives multi-pane layouts across cmux, tmux, and zellij. One keystroke spins up a full dev environment.
4. **Agent-native** — First-class Claude, Codex, and Cursor session tracking. Teleport sessions, archive, and attach — all from the dashboard.

**Terminal mockup**: Screenshots or HTML exports of the real TUI rather than a hand-crafted CSS recreation. More authentic and easier to keep current.

**CTA**: GitHub repo link and install instructions.

### Visual Design

**Palette** (derived from a Google Stitch prototype, a dark nautical/celestial theme):
- Background: `#0d141d` (deep navy)
- Primary: `#f8bd45` (gold)
- Tertiary: `#a9c9f2` (sky blue)
- Surface layers: `#151c25`, `#232a34`, `#2e353f` (tonal depth)
- Text: `#dce3f0` (soft white)

**Typography**:
- Headlines + code: **Recursive** (variable font)
  - Headlines: `CASL ~0.3, MONO ~0.3` — warm, slightly technical
  - Code/terminal samples: `CASL 0, MONO 1` — full monospace
- Body text: **Be Vietnam Pro**

**Style guidelines**:
- Glassmorphism navigation bar (semi-transparent + backdrop blur)
- Tonal surface layering for depth (no 1px borders for sectioning)
- Rounded elements (`xl` or `full` border radius)
- Terminal mockup uses screenshots or HTML exports of the real TUI

## Blog

Astro content collections with Markdown (`.md` / `.mdx`) files in `src/content/blog/`. Low priority at launch — the infrastructure is wired up but content can wait.

Each post needs frontmatter: `title`, `date`, `description`, `tags`.

## Skills and Tooling

- **Anthropic `frontend-design` skill**: Install before implementation to guide distinctive, non-generic page design.
- **Tailwind CSS**: Aligns with the Stitch starting point; custom theme config for the palette and typography.
- **Astro content collections**: Type-safe blog posts with schema validation.

## Future Considerations

- **Docs section**: Deferred until the README stabilizes. Can be added as another Astro content collection or pulled from the main repo.
- **Cloudflare Pages**: Potential migration for preview deploys, faster builds, and flexible routing (proxy `/i/coverage/` etc.). Not needed at launch.
- **Cleat sub-site**: Cleat is moving to its own repo — may need its own section or subdomain later.
- **Service routing**: Top-level URL space is reserved for future services; all generated artifacts stay under `/i/`.
