# Theme Struct with Catppuccin Default Palette

Design spec for issue #217: centralise all hardcoded colours into a `Theme` struct with catppuccin Mocha as the default palette, preserving the current palette as `Theme::classic()`.

## Goals

1. Replace ~100 hardcoded `Color::*` references across the TUI crate with a single `Theme` struct
2. Ship catppuccin Mocha as the default palette
3. Preserve the current palette as `Theme::classic()` for comparison
4. Support runtime theme switching (keybinding to cycle)
5. Accept `--theme` CLI arg and config field for initial theme

## Non-goals

- User-configurable TOML theme files (Phase 3, separate issue)
- Light mode / additional built-in themes beyond classic and catppuccin Mocha
- Theming the glyph showcase example

---

## Theme struct

A flat struct of `Color` fields with semantic names, plus per-site bar styling. Lives in `crates/flotilla-tui/src/theme.rs`.

```rust
use ratatui::style::{Color, Style};

/// How bar labels are transformed before display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    Uppercase,
    Titlecase,
    AsIs,
}

/// Which segment bar renderer to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// Pipe-separated labels: ` Alpha | Beta | Gamma `
    Pipe,
    /// Powerline chevron-delimited chips
    Chevron,
}

/// Per-site bar configuration (tab bar, status bar).
#[derive(Debug, Clone)]
pub struct BarSiteStyle {
    pub kind: BarKind,
    pub label_transform: TextTransform,
}

/// All visual parameters for the TUI.
///
/// Constructed via named constructors (`catppuccin_mocha`, `classic`).
/// Render functions read colour fields directly and call style methods
/// for common patterns.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,

    // ── Chrome ──────────────────────────────────────
    pub tab_active: Color,
    pub tab_inactive: Color,
    pub border: Color,
    pub row_highlight: Color,
    pub multi_select_bg: Color,
    pub section_header: Color,
    pub muted: Color,

    // ── Logo tab ────────────────────────────────────
    pub logo_fg: Color,
    pub logo_bg: Color,
    pub logo_config_bg: Color,

    // ── Work item kinds ─────────────────────────────
    pub checkout: Color,
    pub session: Color,
    pub change_request: Color,
    pub issue: Color,
    pub remote_branch: Color,
    pub workspace: Color,

    // ── Semantic ────────────────────────────────────
    pub branch: Color,
    pub path: Color,
    pub source: Color,
    pub git_status: Color,
    pub error: Color,
    pub warning: Color,
    pub info: Color,

    // ── Interactive ─────────────────────────────────
    pub action_highlight: Color,
    pub input_text: Color,

    // ── Status ──────────────────────────────────────
    pub status_ok: Color,
    pub status_error: Color,

    // ── Surfaces ────────────────────────────────────
    pub base: Color,
    pub surface: Color,
    pub text: Color,
    pub subtext: Color,

    // ── Shimmer ─────────────────────────────────────
    pub shimmer_base: Color,
    pub shimmer_highlight: Color,

    // ── Bar chrome ──────────────────────────────────
    pub bar_bg: Color,
    pub key_hint: Color,
    pub key_chip_bg: Color,
    pub key_chip_fg: Color,

    // ── Bar site styling ────────────────────────────
    pub tab_bar: BarSiteStyle,
    pub status_bar: BarSiteStyle,
}
```

### Constructors

**`Theme::catppuccin_mocha()`** — maps catppuccin Mocha's 26 colours to semantic fields. Uses `catppuccin = { version = "2", features = ["ratatui"] }` for direct `Color` conversion.

**`Theme::classic()`** — reproduces the current hardcoded palette exactly:
- Named colours: `Cyan`, `Blue`, `Green`, `Magenta`, `Yellow`, `Red`, `DarkGray`, `Black`, `White`
- Indexed colours: `203` (status error), `67` (source), `245` (path), `208` (key hint), `236` (multi-select bg)
- Logo tab: fg `Black`, bg `Cyan`, config bg `White`
- Shimmer RGB: `(140, 130, 40)` base, `(255, 240, 120)` highlight
- Git status column: `Red` (distinct from `error` — dirty state, not a fault)

Both constructors set bar site styles:
- `tab_bar`: `Pipe` + `AsIs`
- `status_bar`: `Chevron` + `Uppercase`

### Available themes list

A function returns the ordered list of known themes for cycling:

```rust
pub fn available_themes() -> &'static [fn() -> Theme] {
    &[Theme::catppuccin_mocha, Theme::classic]
}
```

---

## Style methods

Style-producing methods on `Theme` compose colours with modifiers. No caching — the struct is read each frame, making runtime switching immediate.

```rust
impl Theme {
    pub fn logo_style(&self, config_mode: bool) -> Style;
    pub fn tab_style(&self, active: bool, dragging: bool) -> Style;
    pub fn work_item_color(&self, kind: &WorkItemKind) -> Color;
    pub fn status_style(&self, ok: bool) -> Style;
    pub fn header_style(&self) -> Style;
    pub fn log_level_style(&self, level: &str) -> Style;
    pub fn peer_status_style(&self, status: &PeerStatus) -> Style;
    pub fn change_request_status_color(&self, status: &str) -> Color;
    pub fn transform_label(&self, site: &BarSiteStyle, text: &str) -> String;
}
```

Notes:
- `change_request_status_color` takes `&str` because `DeleteSafetyInfo.change_request_status` is `Option<String>`, not the enum.
- `peer_status_style` maps 5 states to 3 colours: Connected → `status_ok`, Disconnected/Rejected → `error`, Connecting/Reconnecting → `warning`.
- `log_level_style` maps ERROR → `error`, WARN → `warning`, INFO → `info`, DEBUG → `branch` (cyan in classic), TRACE → `muted`.
- The shimmer non-truecolor fallback extracts a named colour from `shimmer_highlight`. In classic mode this is `Color::Yellow`; in catppuccin it derives from the RGB value.

Additional methods will emerge during implementation as patterns repeat.

---

## Bar style integration

`TabBarStyle` and `RibbonStyle` in `segment_bar.rs` currently hardcode colours. After this change, they take `&Theme` and read colours from it:

```rust
pub struct ThemedTabBarStyle<'a> {
    theme: &'a Theme,
}

pub struct ThemedRibbonStyle<'a> {
    theme: &'a Theme,
}
```

The rendering code picks which `BarStyle` impl to use based on `theme.tab_bar.kind` and `theme.status_bar.kind`:

```rust
fn make_bar_style<'a>(theme: &'a Theme, site: &BarSiteStyle) -> Box<dyn BarStyle + 'a> {
    match site.kind {
        BarKind::Pipe => Box::new(ThemedTabBarStyle { theme }),
        BarKind::Chevron => Box::new(ThemedRibbonStyle { theme }),
    }
}
```

---

## Theme threading

`Theme` is stored on `App` as `pub theme: Theme`. The `ui::render` signature gains an additional `&Theme` parameter:

```rust
pub fn render(model: &TuiModel, ui: &mut UiState, in_flight: &HashMap<u64, InFlightCommand>, theme: &Theme, frame: &mut Frame)
```

Call site in `run.rs`: `terminal.draw(|f| ui::render(&app.model, &mut app.ui, &app.in_flight, &app.theme, f))`.

Internal render functions (`render_preview`, `render_table`, etc.) receive `&Theme` as a parameter.

---

## Runtime theme switching

Swapping `app.theme` takes effect on the next render tick. No cache invalidation needed.

- **Keybinding:** `T` in Normal mode cycles through `available_themes()`
- **Intent:** `Intent::CycleTheme` flows through the existing dispatch system
- **Status indicator:** Current theme name shown in help screen

---

## CLI and config

**CLI arg:** `--theme <name>` added to the `Cli` struct (clap `#[derive(Parser)]`). Accepts `"catppuccin"` or `"classic"`. Default: `"catppuccin"`.

**Config field:** `theme = "catppuccin"` added to `UiConfig` in `flotilla-core/src/config.rs`. This stores only the theme name string. The TUI crate resolves the name to a `Theme` constructor — the `Theme` type stays in `flotilla-tui`, not `flotilla-core`.

**Precedence:** CLI arg > config file > default (`"catppuccin"`).

---

## Files changed

| File | Change |
|------|--------|
| `crates/flotilla-tui/src/theme.rs` | **New.** `Theme` struct, constructors, style methods, `TextTransform`, `BarKind`, `BarSiteStyle` |
| `crates/flotilla-tui/src/ui.rs` | Replace ~70 hardcoded `Color::*` with `theme.*` and style methods; add `&Theme` param to render functions; update help text with `T` keybinding and theme name |
| `crates/flotilla-tui/src/ui_helpers.rs` | `work_item_icon()` takes `&Theme` |
| `crates/flotilla-tui/src/segment_bar.rs` | `TabBarStyle` / `RibbonStyle` take `&Theme`; add `ThemedTabBarStyle` / `ThemedRibbonStyle` |
| `crates/flotilla-tui/src/shimmer.rs` | Extract RGB from `theme.shimmer_base` / `shimmer_highlight`; derive non-truecolor fallback |
| `crates/flotilla-tui/src/run.rs` | Pass `&app.theme` to `ui::render` |
| `crates/flotilla-tui/src/app/mod.rs` | Store `Theme` on `App`, add `CycleTheme` handling |
| `crates/flotilla-tui/src/app/intent.rs` | Add `Intent::CycleTheme` |
| `crates/flotilla-tui/Cargo.toml` | Add `catppuccin = { version = "2", features = ["ratatui"] }` |
| `src/main.rs` | Add `--theme` CLI arg to `Cli` struct, pass to `App` |
| `crates/flotilla-core/src/config.rs` | Add `theme: String` to `UiConfig` |

### Not changed

- `examples/glyph_showcase.rs` — standalone, not part of app identity
- Protocol / core / client / daemon crates — Theme is TUI-only

---

## Testing

**Snapshot tests:** Update existing snapshots to reflect catppuccin Mocha default. They should match what users see.

**Theme switching test:** Construct `App` with `Theme::catppuccin_mocha()`, render a frame, switch to `Theme::classic()`, render again, assert the rendered output differs. Validates no stale state leaks across switches.

**Classic parity test:** Render with `Theme::classic()` and verify specific cells match the old hardcoded colours (spot checks, not full snapshot comparison).

---

## Design decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| `git_status` vs reusing `error` | Separate field | Git dirty state is not an error; themes may colour them differently |
| Logo tab styling | Dedicated `logo_fg`, `logo_bg`, `logo_config_bg` fields | Logo tab uses style_override with both fg and bg, unlike repo tabs |
| `in_flight` colour field | Removed | Shimmer handles in-flight rendering; no separate single-colour usage exists |
| `row_alt` field | Removed | No alternating row background in the codebase; `Indexed(236)` is multi-select only |
| `base`, `surface`, `text`, `subtext` | Kept | Forward-looking fields for catppuccin surface fills on bordered blocks. In classic mode these map to terminal defaults (Reset/Black/White/DarkGray). Catppuccin uses them for proper surface colouring. |
| `workspace` colour | Added | Workspace indicator column uses Green independently of checkout icon |
| Where Theme lives | `App.theme`, passed as `&Theme` param | Explicit threading over storing on UiState; Theme is not UI state |
| Config bridge | Name string in core, constructor in TUI | Theme type stays TUI-only; config just stores `"catppuccin"` or `"classic"` |

---

## Dependency

```toml
# crates/flotilla-tui/Cargo.toml
catppuccin = { version = "2", features = ["ratatui"] }
```

The `catppuccin` crate provides `catppuccin::PALETTE.mocha.colors` with `From<catppuccin::Color> for ratatui::style::Color`. No other new dependencies.
