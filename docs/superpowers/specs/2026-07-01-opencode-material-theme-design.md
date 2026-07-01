# Design: OpenCode Material Dark Theme

## Context and Problem

Greviewer's color scheme is defined nowhere and everywhere. There is no
theme module. Roughly 150 inline `rgb()`/`rgba()` literals sit in render
functions across `app.rs`, `app/title_bar.rs`, `app/diff_view.rs`,
`app/file_tree.rs`, `app/commit_graph.rs`, `workspace/tab_bar.rs`, and
`workspace/pane_grid.rs`, alongside about 15 per-module `const … u32`
values that never leave the file they live in. The same near-black editor
fill (`0x171717`), the same border grey (`0x242424`/`0x2a2a2a`), and the
same accent blue (`0x7da4ff`) are retyped verbatim in file after file. The
only genuine color object in the tree is the embedded One Dark
`HighlightTheme` in `src/diff_highlight/`, and it governs code-token color
inside diffs only.

Two consequences follow. First, the app has no coherent visual identity:
the sidebar, the main diff surface, and the window chrome all collapse to
the same flat `0x171717`, and the chrome color is whatever gpui-component's
default theme happens to paint because greviewer overrides none of it.
Second, any deliberate change to the look is a scattered find-and-replace
with no guarantee of consistency.

The user wants the app to match their personal Zed theme, *OpenCode
Material Dark* (`~/.config/zed/themes/opencode-material.json`): a
blue-tinted Material Ocean palette with a distinct sidebar surface, a
separate lighter editor surface, blue-grey window chrome, and brighter,
higher-contrast diff status colors than greviewer's current muted fills.

## Requirements and Non-Goals

Requirements:

1. Introduce a single greviewer-owned source of truth for every UI color,
   seeded from *OpenCode Material Dark*. All greviewer-drawn surfaces read
   from it; no view module keeps a private color literal or const.
2. The three surfaces the user called out must be visually distinct and
   correct: the main/editor/diff background is `#263238`, the
   sidebar/panel/tab-bar background is `#1e272c` (darker), and the window
   chrome/title bar is `#1e272c`.
3. The embedded gpui-component widgets greviewer does not paint itself —
   the `TitleBar`, the `Root` background, scrollbars, and inputs — must
   match the palette rather than the library default.
4. Diff status colors gain contrast: added `#c3e88d` and removed `#f07178`
   line backgrounds render at `26` (~15%) alpha, up from the current ~9%,
   matching the Zed theme exactly. Accent, change-kind, and VCS colors
   move to the Material values (accent `#82aaff`, renamed `#89ddff`, etc.).
5. In-diff syntax highlighting is recolored to *OpenCode Material Dark*'s
   `syntax` block, replacing the One Dark palette.
6. Every affected view module keeps at least one `#[gpui::test]`
   (ADR-0003), and new logic (the palette and the gpui-component override)
   carries unit tests. `bin/check` passes clean — zero warnings.

Non-goals: a light theme and light/dark switching (the palette is
structured to admit a light variant later, but none is wired now);
user-configurable theme selection; and any change to layout, spacing, or
typography. This work changes colors only.

## Constraint: ADR-0001

No file may be copied or derived from Zed's GPL-3.0 source. This work is
unaffected: the color values come from the user's own theme JSON (a Zed
*theme file*, which is data, not Zed engine source), and the palette module
and highlight JSON are authored fresh. The existing ADR-0001 note in
`src/diff_highlight/mod.rs` is updated to state the Material colors are the
user's own theme values, authored here, not copied from any GPL source.

## Alternatives Considered

**Retune the literals in place.** Edit the handful of surfaces the user
named — sidebar, main background, chrome, diff status — directly where the
literals live, and stop. This is the fastest path and touches the least
code. It was rejected because it does not solve the actual problem: colors
stay scattered, the next visual change is the same archaeology, and there
is still no way to guarantee that "the editor background" is one value
rather than forty copies of it. The user explicitly chose the centralized
approach over this.

**Adopt gpui-component's `ThemeColor` as the only source.** Delete
greviewer's literals and have every view call `cx.theme().background`,
`cx.theme().sidebar`, and so on, loading a custom `ThemeConfig` JSON at
startup. This aligns with the widgets greviewer already embeds and needs no
new struct. It was rejected as the primary vehicle because greviewer's
semantic needs (diff-line fills at specific alphas, graph lane palettes,
change-kind colors, word-diff emphasis) do not map cleanly onto
gpui-component's field set, and threading `cx` into every color decision —
including pure helpers like `diff_line_fill` that currently take no context
— is more invasive than a plain accessor. We still *use* this mechanism, but
only to push our palette into the library's theme for the embedded widgets
(requirement 3), not as the app-wide source of truth.

**A greviewer-owned palette, with a one-way sync into gpui-component.**
The chosen approach. A small `Palette` struct is the single source of truth
for greviewer's own rendering, and at startup we copy the relevant fields
into gpui-component's global `Theme` so the embedded widgets match. This
keeps greviewer's color decisions context-free and testable while still
unifying the chrome. The trade-off is one synchronization point that must
be kept in step with the palette; a unit test guards it.

## Recommendation and Design

### The palette module — `src/theme.rs`

A new module exposes `Palette`, a plain struct whose fields are
`gpui::Hsla` values with semantic names. It is the single source of truth
for greviewer-drawn color. A module-level accessor returns a shared
instance built once:

```rust
pub fn palette() -> &'static Palette { … } // OnceLock, dark-only
```

Dark-only today. The accessor indirection means a later light variant is a
change inside `theme.rs`, not at the ~150 call sites.

Fields are grouped by role. Exact seed values from *OpenCode Material
Dark*:

- **Surfaces:** `background` `#263238` (main/editor/diff), `surface`
  `#1e272c` (sidebar, panels, tab-bar, chrome), `elevated_surface`
  `#1e272c`.
- **Borders:** `border` `#37474f`, `border_focused` `#82aaff`.
- **Text:** `text` `#eeffff`, `text_muted` `#546e7a`, `text_accent` and
  `accent` `#82aaff`.
- **Tabs:** `tab_bar` `#1e272c`, `tab_active` `#263238`, `tab_inactive`
  `#1e272c`.
- **Interaction:** `element_hover` `#37474fcc`, `selected` `#82aaff33`,
  `drop_target` `#82aaff26`.
- **Scrollbar:** `scrollbar_thumb` `#546e7a40`, `scrollbar_thumb_hover`
  `#546e7a66`.
- **Diff status:** `diff_added_fg` `#c3e88d`, `diff_added_bg` `#c3e88d26`,
  `diff_removed_fg` `#f07178`, `diff_removed_bg` `#f0717826`; word-level
  emphasis retains the stronger `40` alpha it uses today
  (`diff_added_emphasis` `#c3e88d40`, `diff_removed_emphasis` `#f0717840`).
- **Change kinds / VCS:** `added` `#c3e88d`, `modified` `#82aaff`,
  `deleted` `#f07178`, `renamed` `#89ddff`, `conflict` `#f78c6c`, `warning`
  `#ffcb6b`.
- **Graph lanes:** an ordered palette
  `[#82aaff, #c3e88d, #ffcb6b, #c792ea, #89ddff, #f78c6c]` drawn from the
  theme's accent and player colors, replacing the current `PALETTE` const
  in `commit_graph.rs`.

### Syncing the embedded widgets — `lib.rs`

Greviewer does not paint the title bar, the root window background,
scrollbars, or input chrome; gpui-component does, from its global `Theme`.
After `gpui_component::init(cx)` in `run()`, a new
`theme::apply_to_gpui_component(cx)` mutates `Theme::global_mut(cx)` to set
the fields those widgets consume — `background`, `title_bar`,
`title_bar_border`, `border`, the `sidebar*` group, the `tab*` group, the
`scrollbar*` group, and `accent` — from `palette()`. This is a one-way
copy: the palette leads, the library follows.

### Replacing the literals

Module by module, every `rgb()`/`rgba()` literal and every per-module color
const is replaced by a `palette()` field reference, and the now-dead consts
(`TAB_*`, `DIVIDER_*`, `EDGE_HIGHLIGHT_COLOR`, `DIFF_*_EMPHASIS`,
`CURRENT_BRANCH_BG`, `REMOTE_BRANCH_TINT`, `BRANCH_FILTER_MATCH_BG`,
`PALETTE`, and the change-kind maps in `file_tree.rs`) are deleted. Pure
color helpers (`diff_line_fill`, `diff_line_accent`, `change_kind_text`,
`change_kind_border`, `commit_graph_lane_color`) keep their signatures and
simply read from the palette. Where a value has no exact Material
counterpart (e.g. a specific hover tint), it maps to the nearest semantic
palette field rather than inventing a new literal.

### Recoloring syntax — `src/diff_highlight/`

A new embedded `src/diff_highlight/material.json`, in the same
`HighlightTheme` shape as the current `one_dark.json`, carries *OpenCode
Material Dark*'s `syntax` block: `editor.foreground` `#eeffff`, `keyword`
`#c792ea` italic, `string` `#c3e88d`, `function` `#82aaff`,
`number`/`constant`/`boolean` `#f78c6c`, `type` `#ffcb6b`, `property`
`#f07178`, `comment` `#546e7a` italic, `constructor` `#82aaff`,
`operator`/`punctuation` `#89ddff`, `tag` `#f07178`, `title` `#82aaff`
bold, and the remaining keys the highlighter emits. `mod.rs` renames
`one_dark_theme()` to `material_theme()` (with `ONE_DARK` → `MATERIAL`) and
repoints `include_str!`; `one_dark.json` is removed. The module doc comment
is updated per ADR-0001.

## Verification

- **Unit — palette:** assert representative field values (`background`,
  `surface`, `diff_added_bg`, `accent`) resolve to the expected `Hsla`, so
  a mistyped hex is caught.
- **Unit — sync:** in a gpui test context, run
  `apply_to_gpui_component` and assert the mutated `Theme` fields
  (`background`, `title_bar`, `sidebar`, `accent`, a `scrollbar*` field)
  equal the palette, guarding the one synchronization point.
- **Unit — highlight JSON:** `material_theme()` deserializes without panic
  and yields a non-empty style set.
- **View tests:** every affected view module retains its existing
  `#[gpui::test]`; none is removed or weakened.
- **`bin/check`:** `cargo fmt --check`, `cargo clippy --all-targets -- -D
  warnings`, and `cargo test` all pass with zero warnings. No suppression
  directives are introduced.

## Rollout and Follow-up

The change is self-contained and ships in one branch. There is no migration
or persisted state to convert; the theme is compile-time. A natural
follow-up, explicitly out of scope here, is adding the *OpenCode Material
Light* variant and a switch — the palette accessor and the sync function are
the only two places that would need to become mode-aware.
