# OpenCode Material Dark Theme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace greviewer's ~145 scattered color literals and ~15 per-module color constants with a single centralized `Palette` seeded from the *OpenCode Material Dark* Zed theme, sync it into gpui-component's embedded widgets, and recolor in-diff syntax highlighting to match.

**Architecture:** A new `src/theme.rs` owns a `Palette` struct of semantic `Hsla` fields, exposed through a `palette()` accessor backed by a `OnceLock` (dark-only; structured so a light variant can be added later without touching call sites). At startup `apply_to_gpui_component` copies the relevant fields into gpui-component's global `Theme` so the title bar, root background, scrollbars, and inputs match. Every greviewer render site reads from `palette()`; no view keeps a private literal. Syntax highlighting swaps its embedded One Dark JSON for a new Material JSON in the same `HighlightTheme` format.

**Tech Stack:** Rust, gpui, gpui-component 0.5.1 (`ActiveTheme`/`Theme`/`ThemeColor`), `gpui::{rgb, rgba, Hsla}`.

## Global Constraints

- **ADR-0001:** No file may be copied or derived from Zed's GPL-3.0 source. All color values come from the user's own theme file (`~/.config/zed/themes/opencode-material.json`, data not engine source); the palette module and Material highlight JSON are authored fresh here.
- **ADR-0003:** Every view module under `src/` keeps at least one `#[gpui::test]`. No existing view test is removed or weakened. New pure logic carries unit tests.
- **Verification:** `bin/check` (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`) must pass with zero warnings. No suppression directives (`#[allow(...)]`, `#[ignore]`, etc.) may be introduced.
- **Commits:** Conventional Commits (`docs/guides/git.md`). No Claude/Anthropic attribution trailers.
- **Color construction:** Build fields as `Hsla::from(rgb(0xRRGGBB))` for opaque colors and `Hsla::from(rgba(0xRRGGBBAA))` for translucent ones. `rgb`/`rgba` are `gpui` functions; `Hsla: From<Rgba>` exists (already used at `src/app/diff_view.rs:824`).

---

## Palette Field Reference (source of truth for all tasks)

These are the exact fields and seed values `Palette` will carry. Every mapping table in later tasks refers to these field names.

| Field | Source value | Role |
|---|---|---|
| `background` | `rgb(0x263238)` | main content, diff surfaces, graph history, commit rows, active tab, placeholders |
| `surface` | `rgb(0x1e272c)` | branch sidebar, changeset file-list panel, tab-bar strip, popover cards, footers, no-repo card |
| `element_bg` | `rgb(0x37474f)` | neutral element fills (Remove button, Binary badge, inactive icon button) |
| `element_hover` | `rgba(0x37474fcc)` | row hover, section-header hover, pill hover |
| `border` | `rgb(0x37474f)` | all neutral borders, dividers, indent guides, badge borders |
| `text` | `rgb(0xeeffff)` | primary text: names, summaries, headings, values, default tab, badge text, strikethrough bar |
| `text_muted` | `rgb(0x546e7a)` | muted text and icons: hints, counts, timestamps, authors, empty states, separators, folder tint, gutter line numbers, remote-branch tint, graph lane fallback |
| `text_disabled` | `rgba(0x546e7a80)` | unavailable recent-repo path |
| `accent` | `rgb(0x82aaff)` | focus/accent: tab accent line, divider hover, split edge highlight, modified status, accent borders, accent text labels, commit sha in popover, selected commit-row separator |
| `accent_bg` | `rgba(0x82aaff26)` | subtle accent button/pill background |
| `accent_bg_hover` | `rgba(0x82aaff40)` | accent button/pill hover background |
| `row_selected` | `rgba(0x82aaff26)` | selected row background (sidebar, file-list, commit rows) |
| `current_branch_bg` | `rgba(0x82aaff40)` | checked-out branch row background |
| `drop_target` | `rgba(0x82aaff26)` | drag-over drop target tint (tabs, empty pane) |
| `match_highlight_bg` | `rgba(0xffcb6b40)` | branch-filter fuzzy-match highlight |
| `diff_added_fg` | `rgb(0xc3e88d)` | added accent bar, "+N" stats |
| `diff_added_bg` | `rgba(0xc3e88d26)` | added line fill (~15% alpha) |
| `diff_added_emphasis` | `rgba(0xc3e88d40)` | word-level added emphasis |
| `diff_removed_fg` | `rgb(0xf07178)` | removed accent bar, "−N" stats |
| `diff_removed_bg` | `rgba(0xf0717826)` | removed line fill (~15% alpha) |
| `diff_removed_emphasis` | `rgba(0xf0717840)` | word-level removed emphasis |
| `diff_empty_hatch` | `rgba(0x37474f80)` | hatch pattern over alignment gaps |
| `code_text` | `rgb(0xeeffff)` | base code text color under syntax runs |
| `change_added` | `rgb(0xc3e88d)` | change-kind Added (file tree, tab bar) |
| `change_modified` | `rgb(0x82aaff)` | change-kind Modified |
| `change_deleted` | `rgb(0xf07178)` | change-kind Deleted |
| `change_renamed` | `rgb(0x89ddff)` | change-kind Renamed |
| `danger_fg` | `rgb(0xf07178)` | error/unavailable/close-changeset text |
| `danger_bg` | `rgba(0xf0717826)` | danger badge/button background |
| `danger_border` | `rgba(0xf0717866)` | danger badge/button border |
| `commit_hash_fg` | `rgb(0xc3e88d)` | commit short-hash in the graph |
| `ref_head_fg` | `rgb(0x82aaff)` | HEAD ref-label text |
| `ref_head_bg` | `rgba(0x82aaff26)` | HEAD ref-label background |
| `ref_head_border` | `rgba(0x82aaff66)` | HEAD ref-label border |
| `ref_branch_fg` | `rgb(0xc3e88d)` | local-branch ref-label text |
| `ref_branch_bg` | `rgba(0xc3e88d26)` | local-branch ref-label background |
| `ref_branch_border` | `rgba(0xc3e88d66)` | local-branch ref-label border |
| `ref_remote_fg` | `rgb(0x546e7a)` | remote-branch ref-label text |
| `ref_remote_bg` | `rgba(0x546e7a26)` | remote-branch ref-label background |
| `ref_remote_border` | `rgba(0x546e7a66)` | remote-branch ref-label border |
| `graph_lanes` | `[0x82aaff, 0xc3e88d, 0xffcb6b, 0xc792ea, 0x89ddff, 0xf78c6c]` | commit-graph lane palette (`[Hsla; 6]`) |

---

## Task 1: Create the `Palette` module

**Files:**
- Create: `src/theme.rs`
- Modify: `src/lib.rs:9-17` (add `pub mod theme;` to the module list)

**Interfaces:**
- Produces: `pub struct Palette { … }` with all fields from the Palette Field Reference above (all `Hsla`, except `graph_lanes: [Hsla; 6]`); `pub fn palette() -> &'static Palette`.
- Consumes: nothing.

- [ ] **Step 1: Write the failing test**

Add to `src/theme.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_seed_values_match_opencode_material_dark() {
        let p = palette();
        assert_eq!(p.background, Hsla::from(rgb(0x263238)));
        assert_eq!(p.surface, Hsla::from(rgb(0x1e272c)));
        assert_eq!(p.border, Hsla::from(rgb(0x37474f)));
        assert_eq!(p.text, Hsla::from(rgb(0xeeffff)));
        assert_eq!(p.text_muted, Hsla::from(rgb(0x546e7a)));
        assert_eq!(p.accent, Hsla::from(rgb(0x82aaff)));
        assert_eq!(p.diff_added_bg, Hsla::from(rgba(0xc3e88d26)));
        assert_eq!(p.diff_removed_bg, Hsla::from(rgba(0xf0717826)));
        assert_eq!(p.change_renamed, Hsla::from(rgb(0x89ddff)));
        assert_eq!(p.graph_lanes[3], Hsla::from(rgb(0xc792ea)));
    }

    #[test]
    fn palette_returns_the_same_instance() {
        assert!(std::ptr::eq(palette(), palette()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib theme::tests`
Expected: FAIL to compile — `Palette`/`palette` not defined.

- [ ] **Step 3: Write the module**

Create `src/theme.rs`:

```rust
//! Centralized UI color palette for greviewer.
//!
//! Every greviewer-drawn color reads from this module. Values are seeded
//! from the user's *OpenCode Material Dark* Zed theme
//! (`~/.config/zed/themes/opencode-material.json`); the values are authored
//! here and are not copied from any GPL-3.0 source (ADR-0001).
//!
//! Dark-only today. The `palette()` indirection means a future light
//! variant is a change inside this module, not at the call sites.

use std::sync::OnceLock;

use gpui::{rgb, rgba, Hsla};

/// The complete set of semantic UI colors. All fields are `Hsla`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    // Surfaces and structure.
    pub background: Hsla,
    pub surface: Hsla,
    pub element_bg: Hsla,
    pub element_hover: Hsla,
    pub border: Hsla,
    // Text.
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_disabled: Hsla,
    // Accent and interaction.
    pub accent: Hsla,
    pub accent_bg: Hsla,
    pub accent_bg_hover: Hsla,
    pub row_selected: Hsla,
    pub current_branch_bg: Hsla,
    pub drop_target: Hsla,
    pub match_highlight_bg: Hsla,
    // Diff status.
    pub diff_added_fg: Hsla,
    pub diff_added_bg: Hsla,
    pub diff_added_emphasis: Hsla,
    pub diff_removed_fg: Hsla,
    pub diff_removed_bg: Hsla,
    pub diff_removed_emphasis: Hsla,
    pub diff_empty_hatch: Hsla,
    pub code_text: Hsla,
    // Change kinds.
    pub change_added: Hsla,
    pub change_modified: Hsla,
    pub change_deleted: Hsla,
    pub change_renamed: Hsla,
    // Danger.
    pub danger_fg: Hsla,
    pub danger_bg: Hsla,
    pub danger_border: Hsla,
    // Commit graph.
    pub commit_hash_fg: Hsla,
    pub ref_head_fg: Hsla,
    pub ref_head_bg: Hsla,
    pub ref_head_border: Hsla,
    pub ref_branch_fg: Hsla,
    pub ref_branch_bg: Hsla,
    pub ref_branch_border: Hsla,
    pub ref_remote_fg: Hsla,
    pub ref_remote_bg: Hsla,
    pub ref_remote_border: Hsla,
    pub graph_lanes: [Hsla; 6],
}

impl Palette {
    /// The *OpenCode Material Dark* palette.
    fn opencode_material_dark() -> Self {
        Self {
            background: Hsla::from(rgb(0x263238)),
            surface: Hsla::from(rgb(0x1e272c)),
            element_bg: Hsla::from(rgb(0x37474f)),
            element_hover: Hsla::from(rgba(0x37474fcc)),
            border: Hsla::from(rgb(0x37474f)),
            text: Hsla::from(rgb(0xeeffff)),
            text_muted: Hsla::from(rgb(0x546e7a)),
            text_disabled: Hsla::from(rgba(0x546e7a80)),
            accent: Hsla::from(rgb(0x82aaff)),
            accent_bg: Hsla::from(rgba(0x82aaff26)),
            accent_bg_hover: Hsla::from(rgba(0x82aaff40)),
            row_selected: Hsla::from(rgba(0x82aaff26)),
            current_branch_bg: Hsla::from(rgba(0x82aaff40)),
            drop_target: Hsla::from(rgba(0x82aaff26)),
            match_highlight_bg: Hsla::from(rgba(0xffcb6b40)),
            diff_added_fg: Hsla::from(rgb(0xc3e88d)),
            diff_added_bg: Hsla::from(rgba(0xc3e88d26)),
            diff_added_emphasis: Hsla::from(rgba(0xc3e88d40)),
            diff_removed_fg: Hsla::from(rgb(0xf07178)),
            diff_removed_bg: Hsla::from(rgba(0xf0717826)),
            diff_removed_emphasis: Hsla::from(rgba(0xf0717840)),
            diff_empty_hatch: Hsla::from(rgba(0x37474f80)),
            code_text: Hsla::from(rgb(0xeeffff)),
            change_added: Hsla::from(rgb(0xc3e88d)),
            change_modified: Hsla::from(rgb(0x82aaff)),
            change_deleted: Hsla::from(rgb(0xf07178)),
            change_renamed: Hsla::from(rgb(0x89ddff)),
            danger_fg: Hsla::from(rgb(0xf07178)),
            danger_bg: Hsla::from(rgba(0xf0717826)),
            danger_border: Hsla::from(rgba(0xf0717866)),
            commit_hash_fg: Hsla::from(rgb(0xc3e88d)),
            ref_head_fg: Hsla::from(rgb(0x82aaff)),
            ref_head_bg: Hsla::from(rgba(0x82aaff26)),
            ref_head_border: Hsla::from(rgba(0x82aaff66)),
            ref_branch_fg: Hsla::from(rgb(0xc3e88d)),
            ref_branch_bg: Hsla::from(rgba(0xc3e88d26)),
            ref_branch_border: Hsla::from(rgba(0xc3e88d66)),
            ref_remote_fg: Hsla::from(rgb(0x546e7a)),
            ref_remote_bg: Hsla::from(rgba(0x546e7a26)),
            ref_remote_border: Hsla::from(rgba(0x546e7a66)),
            graph_lanes: [
                Hsla::from(rgb(0x82aaff)),
                Hsla::from(rgb(0xc3e88d)),
                Hsla::from(rgb(0xffcb6b)),
                Hsla::from(rgb(0xc792ea)),
                Hsla::from(rgb(0x89ddff)),
                Hsla::from(rgb(0xf78c6c)),
            ],
        }
    }
}

/// Returns the active palette. Built once; dark-only.
pub fn palette() -> &'static Palette {
    static PALETTE: OnceLock<Palette> = OnceLock::new();
    PALETTE.get_or_init(Palette::opencode_material_dark)
}
```

- [ ] **Step 4: Register the module**

In `src/lib.rs`, add `pub mod theme;` alphabetically in the module block (after `pub mod settings;`, before `pub mod window_placement;`):

```rust
pub mod settings;
pub mod theme;
pub mod window_placement;
pub mod workspace;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib theme::tests`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add src/theme.rs src/lib.rs
git commit -m "feat(theme): add centralized OpenCode Material Dark palette"
```

---

## Task 2: Sync the palette into gpui-component

**Files:**
- Modify: `src/theme.rs` (add `apply_to_gpui_component` + test)
- Modify: `src/lib.rs:20-22` (call it after `gpui_component::init`)

**Interfaces:**
- Consumes: `palette()` (Task 1); `gpui_component::Theme`, `gpui::App`.
- Produces: `pub fn apply_to_gpui_component(cx: &mut gpui::App)` — mutates the global `Theme`'s `ThemeColor` fields from the palette.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/theme.rs`:

```rust
    #[gpui::test]
    fn apply_overrides_gpui_component_theme(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            apply_to_gpui_component(cx);

            let theme = <gpui::App as gpui_component::ActiveTheme>::theme(cx);
            let p = palette();
            assert_eq!(theme.background, p.background);
            assert_eq!(theme.title_bar, p.surface);
            assert_eq!(theme.title_bar_border, p.border);
            assert_eq!(theme.sidebar, p.surface);
            assert_eq!(theme.tab_bar, p.surface);
            assert_eq!(theme.tab_active, p.background);
            assert_eq!(theme.accent, p.accent);
            assert_eq!(theme.scrollbar_thumb, Hsla::from(rgba(0x546e7a40)));
        });
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib theme::tests::apply_overrides_gpui_component_theme`
Expected: FAIL to compile — `apply_to_gpui_component` not defined.

- [ ] **Step 3: Write the implementation**

Add to `src/theme.rs` (top-level, after `palette()`), and extend the imports to `use gpui::{rgb, rgba, App, Hsla};`:

```rust
/// Copies the palette into gpui-component's global `Theme` so the widgets
/// greviewer does not paint itself — the title bar, the `Root` window
/// background, scrollbars, and inputs — match the palette. One-way: the
/// palette leads, the library follows. Call once after
/// `gpui_component::init`.
pub fn apply_to_gpui_component(cx: &mut App) {
    let p = palette();
    let theme = gpui_component::Theme::global_mut(cx);

    theme.background = p.background;
    theme.foreground = p.text;
    theme.border = p.border;
    theme.title_bar = p.surface;
    theme.title_bar_border = p.border;
    theme.sidebar = p.surface;
    theme.sidebar_border = p.border;
    theme.sidebar_foreground = p.text;
    theme.tab_bar = p.surface;
    theme.tab = p.surface;
    theme.tab_active = p.background;
    theme.tab_foreground = p.text_muted;
    theme.tab_active_foreground = p.text;
    theme.popover = p.surface;
    theme.popover_foreground = p.text;
    theme.muted_foreground = p.text_muted;
    theme.accent = p.accent;
    theme.ring = p.accent;
    theme.selection = p.row_selected;
    theme.drop_target = p.drop_target;
    theme.input = p.border;
    theme.scrollbar = Hsla::from(rgba(0x00000000));
    theme.scrollbar_thumb = Hsla::from(rgba(0x546e7a40));
    theme.scrollbar_thumb_hover = Hsla::from(rgba(0x546e7a66));
}
```

- [ ] **Step 4: Wire it into startup**

In `src/lib.rs`, immediately after `gpui_component::init(cx);` (line 21):

```rust
        gpui_component::init(cx);
        crate::theme::apply_to_gpui_component(cx);
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib theme::tests`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add src/theme.rs src/lib.rs
git commit -m "feat(theme): sync palette into gpui-component chrome"
```

---

## Task 3: Recolor in-diff syntax highlighting

**Files:**
- Create: `src/diff_highlight/material.json`
- Delete: `src/diff_highlight/one_dark.json`
- Modify: `src/diff_highlight/mod.rs:1-23` (doc comment, `ONE_DARK`→`MATERIAL`, `one_dark_theme`→`material_theme`, `include_str!` target), `:34` (call site), `:195-196` (test name + call)

**Interfaces:**
- Consumes: `gpui_component::highlighter::HighlightTheme`.
- Produces: `pub fn material_theme() -> &'static HighlightTheme` (replaces `one_dark_theme`).

- [ ] **Step 1: Create the Material highlight JSON**

Create `src/diff_highlight/material.json` (values from the *OpenCode Material Dark* `syntax` block):

```json
{
  "name": "OpenCode Material Dark",
  "appearance": "dark",
  "style": {
    "editor.foreground": "#eeffff",
    "syntax": {
      "comment": { "color": "#546e7a", "font_style": "italic" },
      "string": { "color": "#c3e88d" },
      "string.special": { "color": "#c3e88d" },
      "string.escape": { "color": "#eeffff" },
      "keyword": { "color": "#c792ea", "font_style": "italic" },
      "function": { "color": "#82aaff" },
      "number": { "color": "#f78c6c" },
      "constant": { "color": "#f78c6c" },
      "boolean": { "color": "#f78c6c" },
      "type": { "color": "#ffcb6b" },
      "constructor": { "color": "#82aaff" },
      "property": { "color": "#f07178" },
      "attribute": { "color": "#c792ea" },
      "variable": { "color": "#eeffff" },
      "variable.special": { "color": "#f07178" },
      "operator": { "color": "#89ddff" },
      "punctuation": { "color": "#89ddff" },
      "tag": { "color": "#f07178" },
      "embedded": { "color": "#89ddff" },
      "title": { "color": "#82aaff", "font_weight": 600 }
    }
  }
}
```

- [ ] **Step 2: Update the test to the new name and run it (expect fail)**

In `src/diff_highlight/mod.rs`, rename the test at `:195` and its call at `:196`:

```rust
    #[test]
    fn material_theme_parses_from_embedded_json() {
        let theme = material_theme();
```

Run: `cargo test --lib diff_highlight::tests::material_theme_parses_from_embedded_json`
Expected: FAIL to compile — `material_theme` not defined.

- [ ] **Step 3: Rename the loader and repoint the include**

In `src/diff_highlight/mod.rs`, update the doc comment (lines 1-5) to reference the Material palette and ADR-0001 origin, then change lines 15-23:

```rust
static MATERIAL: OnceLock<HighlightTheme> = OnceLock::new();

/// The OpenCode Material Dark highlight theme used by the diff view.
pub fn material_theme() -> &'static HighlightTheme {
    MATERIAL.get_or_init(|| {
        serde_json::from_str(include_str!("material.json"))
            .expect("embedded Material theme JSON is valid")
    })
}
```

Replace the doc comment's One Dark provenance note (lines 3-5) with:

```rust
//! Pure logic only: no gpui rendering. The Material palette is authored
//! from the user's OpenCode Material Dark Zed theme; per ADR-0001 nothing
//! here is copied from Zed's GPL repository.
```

Update the call site at line 34:

```rust
    let theme = material_theme();
```

- [ ] **Step 4: Delete the old JSON**

```bash
git rm src/diff_highlight/one_dark.json
```

- [ ] **Step 5: Run the diff_highlight tests**

Run: `cargo test --lib diff_highlight`
Expected: PASS (all diff_highlight tests, including `material_theme_parses_from_embedded_json`).

- [ ] **Step 6: Commit**

```bash
git add src/diff_highlight/mod.rs src/diff_highlight/material.json
git commit -m "feat(diff): recolor syntax highlighting to OpenCode Material Dark"
```

---

## Task 4: Migrate `diff_view.rs` to the palette

**Files:**
- Modify: `src/app/diff_view.rs` (remove `DIFF_REMOVED_EMPHASIS`/`DIFF_ADDED_EMPHASIS` consts at `:585-586`; replace literals per table)
- Test: `src/app/diff_view.rs` (add pure-function color assertions)

**Interfaces:**
- Consumes: `crate::theme::palette()`.
- Produces: unchanged public signatures for `diff_line_fill`, `diff_line_accent`, `diff_text_style`.

Add `use crate::theme::palette;` to the module imports.

Mapping (replace each literal at the listed line; `p` = `palette()`):

| Line | Old | New |
|---|---|---|
| 43 | `rgb(0x2a2a2a)` | `p.border` |
| 239 | `rgb(0x2a2a2a)` | `p.border` |
| 240 | `rgb(0x1d1d1d)` | `p.surface` |
| 245 | `rgb(0x999999)` | `p.text_muted` |
| 283 | `rgb(0x2a2a2a)` | `p.element_hover` |
| 288 | `rgb(0x999999)` | `p.text_muted` |
| 298 | `rgb(0x171717)` | `p.background` |
| 301 | `rgb(0x999999)` | `p.text_muted` |
| 340 | `rgb(0x171717)` | `p.background` |
| 343 | `rgb(0xfca5a5)` | `p.danger_fg` |
| 621 | `rgba(DIFF_REMOVED_EMPHASIS).into()` | `p.diff_removed_emphasis.into()` |
| 626 | `rgba(DIFF_ADDED_EMPHASIS).into()` | `p.diff_added_emphasis.into()` |
| 696 | `rgb(0x171717)` | `p.background` |
| 748 | `rgb(0x666666)` | `p.text_muted` |
| 812 | `Hsla::from(rgb(0xabb2bf))` | `p.code_text` |
| 824 | `Hsla::from(rgb(0x171717)).into()` | `p.background.into()` |
| 825 | `Hsla::from(rgba(0x98c37918)).into()` | `p.diff_added_bg.into()` |
| 826 | `Hsla::from(rgba(0xe06c7518)).into()` | `p.diff_removed_bg.into()` |
| 827 | `Hsla::from(rgba(0x26262680))` | `p.diff_empty_hatch` |
| 833 | `rgb(0x98c379)` | `p.diff_added_fg` |
| 834 | `rgb(0xe06c75)` | `p.diff_removed_fg` |

Delete lines 585-586 (the two `pub(crate) const` declarations). Update the comment at line 820 to read "Material red/green at ~15% alpha over the editor background".

For each function that references `palette()`, bind `let p = palette();` at the top of the function (or use `palette().field` inline). Where a function is `const`-free and returns colors (`diff_line_fill`, `diff_line_accent`), inline `palette().field` since these are hot paths but cheap (`OnceLock` read).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/app/diff_view.rs`:

```rust
    #[test]
    fn diff_line_fill_uses_palette_diff_backgrounds() {
        use crate::theme::palette;
        let p = palette();
        assert_eq!(diff_line_fill(DiffLineStatus::Added), p.diff_added_bg.into());
        assert_eq!(
            diff_line_fill(DiffLineStatus::Removed),
            p.diff_removed_bg.into()
        );
        assert_eq!(
            diff_line_fill(DiffLineStatus::Unchanged),
            p.background.into()
        );
    }
```

If `diff_line_fill`/`DiffLineStatus` are not in scope for the test module, add the needed `use super::*;` items.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::diff_view::tests::diff_line_fill_uses_palette_diff_backgrounds`
Expected: FAIL — still returns the old `0x171717`/`0x98c379…` values.

- [ ] **Step 3: Apply the mapping edits**

Make every replacement in the table above and delete the two consts.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib app::diff_view`
Expected: PASS (all diff_view tests).

- [ ] **Step 5: Commit**

```bash
git add src/app/diff_view.rs
git commit -m "refactor(diff): read diff-view colors from the palette"
```

---

## Task 5: Migrate `file_tree.rs` to the palette

**Files:**
- Modify: `src/app/file_tree.rs` (`change_kind_border`, `change_kind_text`, and inline literals)
- Test: `src/app/file_tree.rs` (add change-kind color assertions)

**Interfaces:**
- Consumes: `crate::theme::palette()`.
- Produces: unchanged signatures for `change_kind_border`, `change_kind_text`.

Add `use crate::theme::palette;`.

Mapping (`p` = `palette()`):

| Line(s) | Old | New |
|---|---|---|
| 10, 19 | `rgb(0xb8f77a)` (Added) | `p.change_added` |
| 11, 20 | `rgb(0x7da4ff)` (Modified) | `p.change_modified` |
| 12, 21 | `rgb(0xff5f78)` (Deleted) | `p.change_deleted` |
| 13, 22 | `rgb(0xf3d36b)` (Renamed) | `p.change_renamed` |
| 176 | `rgb(0x2b383f)` | `p.border` |
| 208 | `rgb(0xe6eef0)` | `p.text` |
| 223 | `rgb(0xe6eef0)` | `p.text` |
| 297 | `rgb(0xb8f77a)` | `p.diff_added_fg` |
| 302 | `rgb(0xff5f78)` | `p.diff_removed_fg` |

Because `change_kind_border` and `change_kind_text` now return identical values from the palette, keep both functions (call sites differ semantically) but have each read from `palette()`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/app/file_tree.rs`:

```rust
    #[test]
    fn change_kind_colors_come_from_palette() {
        use crate::repo::ChangeKind;
        use crate::theme::palette;
        let p = palette();
        assert_eq!(change_kind_text(ChangeKind::Added), p.change_added);
        assert_eq!(change_kind_text(ChangeKind::Modified), p.change_modified);
        assert_eq!(change_kind_text(ChangeKind::Deleted), p.change_deleted);
        assert_eq!(change_kind_text(ChangeKind::Renamed), p.change_renamed);
    }
```

Match the actual `ChangeKind` path and `change_kind_text` argument type used in the file (adjust `use`/enum variants if the real definition differs).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::file_tree::tests::change_kind_colors_come_from_palette`
Expected: FAIL — returns old `0xb8f77a` etc.

- [ ] **Step 3: Apply the mapping edits**

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib app::file_tree`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/file_tree.rs
git commit -m "refactor(tree): read file-tree colors from the palette"
```

---

## Task 6: Migrate `commit_graph.rs` to the palette

**Files:**
- Modify: `src/app/commit_graph.rs` (`commit_row_separator_color` `:14,16`; ref labels `:100-102`; `PALETTE` const `:589`; fallback `:595`)
- Test: `src/app/commit_graph.rs` (add lane + ref-label assertions)

**Interfaces:**
- Consumes: `crate::theme::palette()`; existing `CommitRefLabelKind`.
- Produces: unchanged signatures for `commit_row_separator_color`, `commit_graph_lane_color`, `render_commit_ref_label`.

Add `use crate::theme::palette;`.

Mapping (`p` = `palette()`):

| Line | Old | New |
|---|---|---|
| 14 | `rgb(0x3b82f6)` (separator selected) | `p.accent` |
| 16 | `rgb(0x242424)` (separator default) | `p.border` |
| 100 | `(rgb(0x0ea5e9), rgb(0x102536), rgb(0x7dd3fc))` HEAD | `(p.ref_head_border, p.ref_head_bg, p.ref_head_fg)` |
| 101 | `(rgb(0x3f6212), rgb(0x17230f), rgb(0xa3e635))` Branch | `(p.ref_branch_border, p.ref_branch_bg, p.ref_branch_fg)` |
| 102 | `(rgb(0x475569), rgb(0x1b2430), rgb(REMOTE_BRANCH_TINT))` Remote | `(p.ref_remote_border, p.ref_remote_bg, p.ref_remote_fg)` |
| 589-595 | `const PALETTE: [u32; 6] = […]; … .unwrap_or_else(|| rgb(0x555555))` | index into `p.graph_lanes`; fallback `p.text_muted` |

For line 102, `REMOTE_BRANCH_TINT` was defined in `app.rs` — after this change it is no longer used by `commit_graph.rs`. (Task 9 removes the `app.rs` const once its sidebar use is also migrated.)

Rewrite `commit_graph_lane_color` (lines ~588-595) to read `palette().graph_lanes`:

```rust
pub(crate) fn commit_graph_lane_color(lane: Option<usize>) -> Rgba /* or existing return type */ {
    let lanes = palette().graph_lanes;
    lane.and_then(|index| lanes.get(index).copied())
        .unwrap_or(palette().text_muted)
        .into() // match the function's existing return type
}
```

Adjust the return-type conversion to match the current signature exactly (read the current function before editing; if it returns `Rgba`, convert via `.into()`; if `Hsla`, drop the conversion). Preserve the existing `lane` parameter shape and the mapping from lane index to color.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/app/commit_graph.rs`:

```rust
    #[test]
    fn lane_colors_come_from_palette() {
        use crate::theme::palette;
        let p = palette();
        // Lane 0 is the first palette entry; match the existing return type.
        assert_eq!(commit_graph_lane_color(Some(0)), p.graph_lanes[0].into());
        assert_eq!(commit_graph_lane_color(Some(4)), p.graph_lanes[4].into());
    }
```

Adjust the argument form and `.into()` to match the real `commit_graph_lane_color` signature.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::commit_graph::tests::lane_colors_come_from_palette`
Expected: FAIL — returns old `PALETTE` values.

- [ ] **Step 3: Apply the mapping edits**

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib app::commit_graph`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/commit_graph.rs
git commit -m "refactor(graph): read commit-graph colors from the palette"
```

---

## Task 7: Migrate `tab_bar.rs` and `pane_grid.rs` to the palette

**Files:**
- Modify: `src/workspace/tab_bar.rs` (remove `TAB_*` consts `:25-30`; replace usages + `:107`)
- Modify: `src/workspace/pane_grid.rs` (remove `DIVIDER_*`/`EDGE_HIGHLIGHT_COLOR` consts `:26,27,96`; replace usages + `:131`)

**Interfaces:**
- Consumes: `crate::theme::palette()`.
- Produces: no signature changes.

Add `use crate::theme::palette;` to both files.

`tab_bar.rs` — delete the six consts (`:25-30`) and replace every reference to them, plus line 107:

| Const / line | Old value | New (`palette()` field) |
|---|---|---|
| `TAB_BAR_BG` | `0x111111` | `surface` |
| `TAB_ACTIVE_BG` | `0x171717` | `background` |
| `TAB_BORDER` | `0x2a2a2a` | `border` |
| `TAB_ACCENT` | `0x7da4ff` | `accent` |
| `TAB_MUTED_TEXT` | `0x8a8a8a` | `text_muted` |
| `TAB_DEFAULT_TEXT` | `0xe6eef0` | `text` |
| line 107 `rgb(0x1d2733)` | drag-over | `drop_target` |

Locate every use of the deleted consts (they are referenced elsewhere in the file's render code) via `rg -n "TAB_BAR_BG|TAB_ACTIVE_BG|TAB_BORDER|TAB_ACCENT|TAB_MUTED_TEXT|TAB_DEFAULT_TEXT" src/workspace/tab_bar.rs` and replace each with the corresponding `palette().<field>`.

`pane_grid.rs` — delete the three consts and replace usages, plus line 131:

| Const / line | Old value | New |
|---|---|---|
| `DIVIDER_COLOR` | `0x2a2a2a` | `border` |
| `DIVIDER_HOVER_COLOR` | `0x7da4ff` | `accent` |
| `EDGE_HIGHLIGHT_COLOR` | `0x7da4ff` | `accent` |
| line 131 `rgb(0x1d2733)` | drag-over | `drop_target` |

Find usages via `rg -n "DIVIDER_COLOR|DIVIDER_HOVER_COLOR|EDGE_HIGHLIGHT_COLOR" src/workspace/pane_grid.rs` and replace each.

- [ ] **Step 1: Apply the edits to both files**

Delete the consts and replace all usages per the tables. These modules' colors have no pure accessor returning them, so verification is compile + existing view tests.

- [ ] **Step 2: Verify compilation and existing tests**

Run: `cargo test --lib workspace`
Expected: PASS (existing `tab_bar`/`pane_grid` view tests). No `unused const` or `unused import` warnings.

- [ ] **Step 3: Commit**

```bash
git add src/workspace/tab_bar.rs src/workspace/pane_grid.rs
git commit -m "refactor(workspace): read tab and pane colors from the palette"
```

---

## Task 8: Migrate `title_bar.rs` to the palette

**Files:**
- Modify: `src/app/title_bar.rs` (`:127-128`, `:155`, `:171`, `:175`, `:232-496` per table)

**Interfaces:**
- Consumes: `crate::theme::palette()`.
- Produces: no signature changes. Note `switcher_pill` takes a `u32` text-color argument at `:155`/`:175`; convert those call sites to pass the palette color (change the parameter type to `Hsla` or convert at the call site — read `switcher_pill`'s signature first and pick the smaller change).

Add `use crate::theme::palette;`.

Mapping (`p` = `palette()`):

| Line | Old | New |
|---|---|---|
| 127 | `rgb(0x2a2a2a)` | `p.element_hover` |
| 128 | `rgb(0x3a3a3a)` | `p.element_bg` |
| 155 | `0xe6e6e6` (pill text arg) | `p.text` |
| 171 | `rgb(0x5a5a5a)` | `p.text_muted` |
| 175 | `0xdbeafe` (pill text arg) | `p.accent` |
| 232 | `rgb(0xededed)` | `p.text` |
| 240 | `rgb(0x8a8a93)` | `p.text_muted` |
| 253 | `rgb(0x8a8a93)` | `p.text_muted` |
| 260 | `rgb(0xc7c7cf)` | `p.text` |
| 269 | `rgb(0x7ee787)` | `p.diff_added_fg` |
| 272 | `rgb(0xf08a8a)` | `p.diff_removed_fg` |
| 288 | `rgb(0x5a2a2a)` | `p.danger_border` |
| 289 | `rgb(0x2a1818)` | `p.danger_bg` |
| 291 | `rgb(0xf3b4b4)` | `p.danger_fg` |
| 303 | `rgb(0x26262c)` | `p.border` |
| 305 | `rgb(0x8a8a93)` | `p.text_muted` |
| 320 | `rgb(0x26262c)` | `p.border` |
| 334 | `rgb(0x7aa2f7)` | `p.accent` |
| 341 | `rgb(0xc7c7cf)` | `p.text` |
| 355 | `rgb(0x141417)` | `p.surface` |
| 357 | `rgb(0x34343a)` | `p.border` |
| 412 | `rgb(0x26262c)` | `p.border` |
| 415 | `rgb(0x8a8a93)` | `p.text_muted` |
| 425 | `rgb(0x8a8a93)` | `p.text_muted` |
| 446 | `rgb(0xc7c7cf)` | `p.text` |
| 455 | `rgb(0x8a8a93)` | `p.text_muted` |
| 478 | `rgb(0x26262c)` | `p.border` |
| 480 | `rgb(0xdbeafe)` | `p.accent` |
| 494 | `rgb(0x141417)` | `p.surface` |
| 496 | `rgb(0x34343a)` | `p.border` |

For `switcher_pill` (`:155`, `:175`): if its signature is `fn switcher_pill(id: …, text_color: u32, open: bool)`, change `text_color` to `impl Into<Hsla>` (or `Hsla`) and pass `p.text` / `p.accent`. Update the two call sites accordingly.

- [ ] **Step 1: Apply the edits**

- [ ] **Step 2: Verify compilation and existing tests**

Run: `cargo test --lib app::title_bar`
Expected: PASS. No unused-import or type-mismatch warnings.

- [ ] **Step 3: Commit**

```bash
git add src/app/title_bar.rs
git commit -m "refactor(ui): read title-bar colors from the palette"
```

---

## Task 9: Migrate `app.rs` to the palette

This is the largest surface. The key judgment: split the shared `0x171717` by layout region — **sidebar and changeset file-list surfaces become `surface` (#1e272c); main content (graph history, changeset root, commit rows, placeholders) becomes `background` (#263238)** — so the left column reads uniformly darker than the main area, matching the target theme.

**Files:**
- Modify: `src/app.rs` (remove consts `:112`, `:3488`, `:3494`; replace literals per tables; add uniform sidebar/file-list container backgrounds)

**Interfaces:**
- Consumes: `crate::theme::palette()`.
- Produces: no signature changes.

Add `use crate::theme::palette;` to `app.rs` imports.

### 9a. Delete the three consts

- `:112` `BRANCH_FILTER_MATCH_BG` — replace its use at `:2251` (`rgba(BRANCH_FILTER_MATCH_BG).into()`) with `palette().match_highlight_bg.into()`, then delete the const.
- `:3488` `REMOTE_BRANCH_TINT` — its uses are `:2291` (sidebar remote row text) → `palette().ref_remote_fg` (same `#546e7a` family) and the already-migrated `commit_graph.rs:102`. After Task 6 and the `:2291` edit, delete the const.
- `:3494` `CURRENT_BRANCH_BG` — replace its use at `:2284` with `palette().current_branch_bg`, then delete the const.

### 9b. Main-content region → `background`

| Line | Old | New |
|---|---|---|
| 1946 | `rgb(0x171717)` (history panel) | `palette().background` |
| 1981 | `rgb(0x171717)` (graph screen root) | `palette().background` |
| 2618 | `rgb(0x171717)` (changeset screen root) | `palette().background` |
| 3398 | `rgb(0x171717)` (commit row default) | `palette().background` |

### 9c. Sidebar + file-list region → `surface`

| Line | Old | New | Region |
|---|---|---|---|
| 2286 | `rgb(0x171717)` (branch row default) | `palette().surface` | sidebar |
| 2424 | `rgb(0x171717)` (section row) | `palette().surface` | sidebar |
| 2501 | `rgb(0x171717)` (folder row) | `palette().surface` | sidebar |
| 2855 | `rgb(0x171717)` (file-tree repo header) | `palette().surface` | file-list |
| 3005 | `rgb(0x171717)` (gutter cell unselected) | `palette().surface` | file-list |
| 3057 | `rgb(0x171717)` (folder row) | `palette().surface` | file-list |
| 3095 | `rgb(0x171717)` (changed file row default) | `palette().surface` | file-list |
| 3188 | `rgb(0x171717)` (unchanged file row default) | `palette().surface` | file-list |

Additionally, to keep the left columns uniform where rows do not fill the pane:
- Branch-sidebar container (`:2185`, currently only `.border_color(rgb(0x242424))`): add `.bg(palette().surface)`.
- File-list root container: give the `render_file_list` root the background `palette().surface`. Read `render_file_list` (from `:2653`) and set `.bg(palette().surface)` on its outermost returned `div`; if the root is assembled downstream, set it on the outermost container that wraps the rows.

### 9d. Selection / hover / current-branch

| Line | Old | New |
|---|---|---|
| 2282 | `rgb(0x223248)` (branch row selected) | `palette().row_selected` |
| 2284 | `rgb(CURRENT_BRANCH_BG)` | `palette().current_branch_bg` |
| 2321 | `rgb(0x1f2733)` (row hover) | `palette().element_hover` |
| 2431 | `rgb(0x1f2733)` (section hover) | `palette().element_hover` |
| 2506 | `rgb(0x1f2733)` (folder hover) | `palette().element_hover` |
| 3003 | `rgb(0x223248)` (gutter selected) | `palette().row_selected` |
| 3093 | `rgb(0x223248)` (changed file selected) | `palette().row_selected` |
| 3186 | `rgb(0x223248)` (unchanged file selected) | `palette().row_selected` |
| 3396 | `rgb(0x223248)` (commit row selected) | `palette().row_selected` |
| 2251 | `rgba(BRANCH_FILTER_MATCH_BG)` | `palette().match_highlight_bg` |

### 9e. Accent family (buttons, icon buttons)

| Line | Old | New |
|---|---|---|
| 1962 | `rgb(0x3b82f6)` (button border) | `palette().accent` |
| 1963 | `rgb(0x1d283a)` (button bg) | `palette().accent_bg` |
| 1964 | `rgb(0xdbeafe)` (button text) | `palette().accent` |
| 2932 (active) | `rgb(0x1d283a)` | `palette().accent_bg` |
| 2932 (inactive) | `rgb(0x202020)` | `palette().element_bg` |
| 2933 (active) | `rgb(0xdbeafe)` | `palette().accent` |
| 2933 (inactive) | `rgb(0x999999)` | `palette().text_muted` |
| 2946 (hover bg) | `rgb(0x2c3a4f)` | `palette().accent_bg_hover` |
| 2946 (hover text) | `rgb(0xdbeafe)` | `palette().accent` |

### 9f. Borders, text, danger, misc

| Line | Old | New |
|---|---|---|
| 1685 | `rgb(0x2a2a2a)` | `palette().border` |
| 1686 | `rgb(0x141414)` | `palette().surface` |
| 1692 | `rgb(0x242424)` | `palette().border` |
| 1693 | `rgb(0x999999)` | `palette().text_muted` |
| 1720 | `rgb(0xe6e6e6)` | `palette().text` |
| 1726 | `rgb(0x999999)` | `palette().text_muted` |
| 1749 | `rgb(0xe6e6e6)` | `palette().text` |
| 1751 | `rgb(0x777777)` | `palette().text_disabled` |
| 1763 | `rgb(0x242424)` | `palette().border` |
| 1789 | `rgb(0x5a2a2a)` | `palette().danger_border` |
| 1790 | `rgb(0x241818)` | `palette().danger_bg` |
| 1791 | `rgb(0xfca5a5)` | `palette().danger_fg` |
| 1800 | `rgb(0x3a3a3a)` | `palette().border` |
| 1801 | `rgb(0x1f1f1f)` | `palette().element_bg` |
| 1802 | `rgb(0xbdbdbd)` | `palette().text` |
| 1892 | `rgb(0x999999)` | `palette().text_muted` |
| 2073 | `rgb(0x242424)` | `palette().border` |
| 2078 | `rgb(0x999999)` | `palette().text_muted` |
| 2104 | `rgb(0x999999)` | `palette().text_muted` |
| 2118 | `rgb(0x999999)` | `palette().text_muted` |
| 2137 | `rgb(0x999999)` | `palette().text_muted` |
| 2185 | `rgb(0x242424)` | `palette().border` (plus add `.bg(palette().surface)`, see 9c) |
| 2289 | `rgb(0x999999)` | `palette().text_muted` |
| 2291 | `rgb(REMOTE_BRANCH_TINT)` | `palette().ref_remote_fg` |
| 2293 | `rgb(0xe6e6e6)` | `palette().text` |
| 2392 | `rgb(0x999999)` | `palette().text_muted` |
| 2427 | `rgb(0x242424)` | `palette().border` |
| 2441 | `rgb(0x999999)` | `palette().text_muted` |
| 2452 | `rgb(0x999999)` | `palette().text_muted` |
| 2460 | `rgb(0x999999)` | `palette().text_muted` |
| 2468 | `rgb(0x999999)` | `palette().text_muted` |
| 2489 (hidden) | `rgb(0x999999)` | `palette().text_muted` |
| 2489 (default) | `rgb(0x8aa6bd)` | `palette().text_muted` |
| 2574 | `rgb(0x999999)` | `palette().text_muted` |
| 2669 | `rgb(0x999999)` | `palette().text_muted` |
| 2755 | `rgb(0x242424)` | `palette().border` |
| 2861 | `rgb(0x8aa6bd)` | `palette().text_muted` |
| 2868 | `rgb(0x8aa6bd)` | `palette().text_muted` |
| 3068 | `rgb(0x8aa6bd)` | `palette().text_muted` |
| 3073 | `rgb(0x8aa6bd)` | `palette().text_muted` |
| 3152 | `rgb(0x525252)` | `palette().border` |
| 3153 | `rgb(0x242424)` | `palette().element_bg` |
| 3154 | `rgb(0xbdbdbd)` | `palette().text` |
| 3164 | `rgb(0x8a8a8a)` | `palette().text_muted` |
| 3220 | `rgb(0x6f7d87)` | `palette().text_muted` |
| 3225 | `rgb(0xb8c0c7)` | `palette().text` |
| 3258 | `rgb(0x999999)` | `palette().text_muted` |
| 3344 | `rgb(0x2a2a2a)` | `palette().border` |
| 3345 | `rgb(0x999999)` | `palette().text_muted` |
| 3442 | `rgb(0xa3e635)` | `palette().commit_hash_fg` |
| 3452 | `rgb(0xe6e6e6)` | `palette().text` |
| 3463 | `rgb(0xa3a3a3)` | `palette().text_muted` |
| 3473 | `rgb(0x8a8a8a)` | `palette().text_muted` |

Where a conditional expression selects between two literals (e.g. `:2282`/`:2286`, `:2489`, `:2932`/`:2933`), replace both branches per the tables above.

- [ ] **Step 1: Apply 9a (consts + their usages)**

Replace the three consts' usages, then delete the consts. Run `rg -n "BRANCH_FILTER_MATCH_BG|REMOTE_BRANCH_TINT|CURRENT_BRANCH_BG" src/` and confirm zero matches remain across the whole crate.

- [ ] **Step 2: Apply 9b–9f (all literal replacements + container backgrounds)**

Work top-to-bottom through the tables. After finishing, run `rg -n "rgb\(0x|rgba\(0x" src/app.rs` and confirm zero matches remain.

- [ ] **Step 3: Verify compilation and existing app tests**

Run: `cargo test --lib app`
Expected: PASS (all existing `app` view/unit tests). No unused-import, unused-const, or type warnings.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "refactor(app): read app-shell colors from the palette"
```

---

## Task 10: Final verification and cleanup

**Files:**
- Verify only; fix any residue found.

- [ ] **Step 1: Confirm no color literals or dead color consts remain**

Run: `rg -n "rgb\(0x|rgba\(0x" src/`
Expected: matches appear **only** in `src/theme.rs` (the palette definitions and the two `scrollbar` values inside `apply_to_gpui_component`). Any match elsewhere is a missed migration — fix it and re-run.

Run: `rg -n "one_dark" src/`
Expected: zero matches.

- [ ] **Step 2: Run the full check suite**

Run: `bin/check`
Expected: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean (zero warnings), `cargo test` all green across unit/integration/view/smoke levels.

If clippy flags anything (e.g. an import that became unused after a literal was removed), fix the underlying cause — do not suppress.

- [ ] **Step 3: Manual visual sanity check**

Run: `cargo run` and confirm against the target theme:
- Window chrome / title bar is blue-grey `#1e272c`.
- The left column (branch sidebar / changeset file list) is `#1e272c` — visibly darker than the main area.
- The main graph/diff area is `#263238`.
- Added lines are green `#c3e88d` and removed lines red `#f07178`, with noticeably stronger fills than before.
- Selected rows read blue-tinted; the checked-out branch reads a stronger blue.
- Syntax highlighting inside a diff shows Material colors (purple italic keywords, green strings, blue functions).

- [ ] **Step 4: Update the design spec status if needed and finalize**

No code change expected here. If the visual check surfaced any deviation from the spec, correct the palette value in `src/theme.rs`, re-run `bin/check`, and amend the relevant task's commit is not allowed — make a new fix commit:

```bash
git add -A
git commit -m "fix(theme): correct <field> to match OpenCode Material Dark"
```

---

## Self-Review

**Spec coverage:** Requirement 1 (single source of truth) → Tasks 1, 4–9 + Task 10 Step 1 guard. Requirement 2 (distinct sidebar/main/chrome surfaces) → Task 9c + Task 2 (chrome). Requirement 3 (embedded widgets) → Task 2. Requirement 4 (diff contrast + accent/VCS values) → Tasks 4, 5, 8 + palette values. Requirement 5 (syntax recolor) → Task 3. Requirement 6 (tests + clean check) → per-task tests + Task 10. Non-goals (light theme, switching, config UI, layout) → untouched. ADR-0001 note → Task 3 Step 3.

**Placeholder scan:** No TBD/TODO. The only deliberately deferred detail is matching two existing function signatures (`commit_graph_lane_color` return type in Task 6; `switcher_pill` text-color parameter in Task 8) — both instruct reading the current signature first and give the exact edit for each shape.

**Type consistency:** `palette()` returns `&'static Palette`; all fields `Hsla` (except `graph_lanes: [Hsla; 6]`). `apply_to_gpui_component(cx: &mut App)` matches Task 2's interface and Task 10's expectations. Field names in every mapping table match the Palette Field Reference and the struct in Task 1. `material_theme()` name is used consistently in Task 3.
