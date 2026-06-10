# Diff Workspace Tabs — Slice 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-diff detail pane in the changeset review screen with one tabbed pane: single-click opens a preview tab, double-click pins, tabs close via hover-× and middle-click, styled after Zed.

**Architecture:** A new `src/workspace/` module holds a pure-state `Workspace`/`Pane` machine (unit-testable without gpui) plus a `tab_bar` render submodule. The root `App` view owns a `Workspace` value; `App.selected_changed_file_path` is removed and the workspace becomes the source of truth for what the detail area shows. A separate `file_tree_highlight_path` keeps the tree highlight click-driven. Spec: `docs/superpowers/specs/2026-06-09-diff-workspace-tabs-design.md` (this plan implements phase/slice 1 only).

**Tech Stack:** Rust, gpui 0.2.2, gpui-component 0.5, git2 (tests). Verification: `bin/check`.

---

## Verified API facts (do not re-derive)

- `gpui::ClickEvent` is an **enum** (`Mouse`/`Keyboard`) with a public method `click_count(&self) -> usize`. Double-click detection in an `.on_click` listener: `event.click_count() >= 2`. A physical double-click delivers TWO click events: first with count 1, then with count 2. Our semantics rely on this: click 1 opens the preview, click 2 pins it in place.
- `VisualTestContext::simulate_click(position, modifiers)` always sends `click_count: 1`. There is no public double-click helper; `Window::dispatch_event(PlatformInput, cx)` IS public, so we add our own test helper (Task 3).
- Middle-click in tests: `visual.simulate_mouse_down(pos, MouseButton::Middle, Modifiers::none())` then `simulate_mouse_up(...)` — these accept any button.
- `ScrollHandle::scroll_to_item(index)` exists and scrolls the indexed child of the tracked element into view.
- `cx.stop_propagation()` is available inside listeners (prevents a child's click from also triggering the parent tab's click handler).
- gpui `Div` styling used below: `.italic()`, `.opacity(f32)`, `.group(name)` / `.group_hover(name, |s| ...)`, `.overflow_x_scroll()`, `.track_scroll(&ScrollHandle)` (requires `.id(...)`), `.debug_selector(move || ...)`.
- Existing colors/conventions: panel bg `0x171717`, hairline `0x2a2a2a` family, file-name text `0xe6eef0`, muted `0x8a8a8a`, change-kind colors via `change_kind_text(kind)` in `src/app.rs:3897` (currently private). Font: `BerkeleyMono Nerd Font`.
- Test conventions: `#[gpui::test] async fn name(cx: &mut TestAppContext)`, fixtures via git2 tempdir repos, `cx.run_until_parked()` between interactions, `VisualTestContext::from_window(*window, cx)`, `debug_bounds("selector")`. See `src/app.rs:8475` (`changed_file_row_selection_via_gutter…` test) for the open-repo → select-commit → click `open-changeset` pattern.
- Commit style (`docs/guides/git.md`): conventional commits, e.g. `feat(workspace): …`.

---

### Task 1: Vendor the Lucide "x" icon (tab close glyph)

**Files:**
- Create: `assets/icons/x.svg`
- Modify: `src/icons.rs`

- [ ] **Step 1: Add the SVG asset**

Create `assets/icons/x.svg` (Lucide is ISC-licensed; matches the formatting of the existing vendored icons):

```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M18 6 6 18" />
  <path d="m6 6 12 12" />
</svg>
```

- [ ] **Step 2: Add the enum variant**

In `src/icons.rs`, add to the `LucideIcon` enum (alphabetical position is last):

```rust
    /// `x.svg`
    X,
```

Add `LucideIcon::X,` to the `ALL` const array, and to the `path()` match:

```rust
            LucideIcon::X => "icons/x.svg",
```

- [ ] **Step 3: Run the icon asset test**

Run: `cargo test --lib icons`
Expected: PASS (the existing test iterates `ALL` and asserts each SVG asset exists).

- [ ] **Step 4: Commit**

```bash
git add assets/icons/x.svg src/icons.rs
git commit -m "feat(icons): vendor lucide x icon for tab close buttons"
```

---

### Task 2: Workspace state machine — items, preview/pinned open semantics

**Files:**
- Create: `src/workspace/mod.rs`
- Modify: `src/lib.rs` (add `pub mod workspace;` alongside the existing module declarations)

- [ ] **Step 1: Create the module with types and failing tests**

Create `src/workspace/mod.rs`:

```rust
//! Tabbed workspace state for the changeset review screen.
//!
//! Implements slice 1 of `docs/superpowers/specs/2026-06-09-diff-workspace-tabs-design.md`:
//! a single pane holding preview and pinned tabs. Later slices add splits,
//! drag and drop, and layout persistence on top of this module without
//! changing its public API. All state transitions here are pure and
//! unit-tested without gpui; rendering lives in `tab_bar`.

pub mod tab_bar;

/// Anything that can occupy a tab.
pub trait WorkspaceItem {
    /// Stable identity within a pane. Opening an item whose key is already
    /// present activates the existing tab instead of adding a duplicate.
    fn key(&self) -> &str;
    /// The file-name portion shown as the tab title.
    fn tab_title(&self) -> &str;
    /// Full repository-relative path; drives duplicate-title disambiguation
    /// and tells the detail pane what to render.
    fn path(&self) -> &str;
}

/// A file diff (or read-only file view) open in a tab.
pub struct FileDiffItem {
    path: String,
}

impl FileDiffItem {
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

impl WorkspaceItem for FileDiffItem {
    fn key(&self) -> &str {
        &self.path
    }

    fn tab_title(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    fn path(&self) -> &str {
        &self.path
    }
}

/// One tab strip plus its contents. Slice 1 has exactly one pane; the
/// workspace API already speaks in terms of "the active pane" so that later
/// slices can add more panes without changing callers.
struct Pane {
    tabs: Vec<Box<dyn WorkspaceItem>>,
    active: Option<usize>,
    /// Index of the preview tab, if one exists. At most one per pane.
    preview: Option<usize>,
}

pub struct Workspace {
    pane: Pane,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            pane: Pane {
                tabs: Vec::new(),
                active: None,
                preview: None,
            },
        }
    }

    pub fn tabs(&self) -> &[Box<dyn WorkspaceItem>] {
        &self.pane.tabs
    }

    pub fn active_index(&self) -> Option<usize> {
        self.pane.active
    }

    pub fn active_item(&self) -> Option<&dyn WorkspaceItem> {
        self.pane.active.map(|index| self.pane.tabs[index].as_ref())
    }

    pub fn is_preview(&self, index: usize) -> bool {
        self.pane.preview == Some(index)
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.pane.tabs.iter().position(|item| item.key() == key)
    }

    /// Open `item` in the preview slot. Returns true when the active item's
    /// content changed (callers reset diff scroll on true).
    pub fn open_preview(&mut self, item: Box<dyn WorkspaceItem>) -> bool {
        if let Some(index) = self.find(item.key()) {
            return self.activate_tab(index);
        }
        match self.pane.preview {
            Some(index) => {
                self.pane.tabs[index] = item;
                self.pane.active = Some(index);
                true
            }
            None => {
                self.pane.tabs.push(item);
                let index = self.pane.tabs.len() - 1;
                self.pane.preview = Some(index);
                self.pane.active = Some(index);
                true
            }
        }
    }

    /// Open `item` pinned. An existing tab for the same key is activated and
    /// promoted out of preview. Returns true when the active item changed.
    pub fn open_pinned(&mut self, item: Box<dyn WorkspaceItem>) -> bool {
        if let Some(index) = self.find(item.key()) {
            if self.pane.preview == Some(index) {
                self.pane.preview = None;
            }
            return self.activate_tab(index);
        }
        self.pane.tabs.push(item);
        self.pane.active = Some(self.pane.tabs.len() - 1);
        true
    }

    /// Returns true when the active item changed.
    pub fn activate_tab(&mut self, index: usize) -> bool {
        if index >= self.pane.tabs.len() || self.pane.active == Some(index) {
            return false;
        }
        self.pane.active = Some(index);
        true
    }

    /// Promote the tab at `index` out of preview, if it is the preview tab.
    pub fn promote_tab(&mut self, index: usize) {
        if self.pane.preview == Some(index) {
            self.pane.preview = None;
        }
    }

    /// Close the tab at `index`. The right neighbor (or left, at the end of
    /// the strip) becomes active. Returns true when the active item changed.
    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.pane.tabs.len() {
            return false;
        }
        self.pane.tabs.remove(index);
        match self.pane.preview {
            Some(preview) if preview == index => self.pane.preview = None,
            Some(preview) if preview > index => self.pane.preview = Some(preview - 1),
            _ => {}
        }
        let previous_active = self.pane.active;
        self.pane.active = if self.pane.tabs.is_empty() {
            None
        } else {
            match previous_active {
                Some(active) if active == index => Some(index.min(self.pane.tabs.len() - 1)),
                Some(active) if active > index => Some(active - 1),
                other => other,
            }
        };
        previous_active == Some(index)
    }

    /// Close every tab. Used when leaving a changeset.
    pub fn clear(&mut self) {
        self.pane.tabs.clear();
        self.pane.active = None;
        self.pane.preview = None;
    }
}
```

- [ ] **Step 2: Write the unit tests for open/activate semantics**

Append to `src/workspace/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str) -> Box<dyn WorkspaceItem> {
        Box::new(FileDiffItem::new(path.to_string()))
    }

    fn paths(workspace: &Workspace) -> Vec<&str> {
        workspace.tabs().iter().map(|tab| tab.path()).collect()
    }

    #[test]
    fn file_diff_item_title_is_file_name() {
        assert_eq!(FileDiffItem::new("a/b/c.rs".into()).tab_title(), "c.rs");
        assert_eq!(FileDiffItem::new("top.rs".into()).tab_title(), "top.rs");
    }

    #[test]
    fn open_preview_creates_a_single_preview_tab() {
        let mut ws = Workspace::new();
        assert!(ws.open_preview(item("a.rs")));
        assert_eq!(paths(&ws), ["a.rs"]);
        assert_eq!(ws.active_index(), Some(0));
        assert!(ws.is_preview(0));
    }

    #[test]
    fn second_preview_replaces_the_first_in_place() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(ws.open_preview(item("b.rs")));
        assert_eq!(paths(&ws), ["b.rs"]);
        assert!(ws.is_preview(0));
        assert_eq!(ws.active_index(), Some(0));
    }

    #[test]
    fn preview_keeps_its_strip_position_when_replaced() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("pinned.rs"));
        ws.open_preview(item("a.rs"));
        ws.activate_tab(0);
        ws.open_preview(item("b.rs"));
        assert_eq!(paths(&ws), ["pinned.rs", "b.rs"]);
        assert!(ws.is_preview(1));
        assert_eq!(ws.active_index(), Some(1));
    }

    #[test]
    fn opening_an_already_open_file_activates_instead_of_duplicating() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        assert!(ws.open_preview(item("a.rs")));
        assert_eq!(paths(&ws), ["a.rs", "b.rs"]);
        assert_eq!(ws.active_index(), Some(0));
        assert!(!ws.is_preview(0), "pinned tab must stay pinned");
    }

    #[test]
    fn reopening_the_active_file_reports_no_change() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(!ws.open_preview(item("a.rs")));
    }

    #[test]
    fn open_pinned_appends_and_leaves_preview_alone() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(ws.open_pinned(item("b.rs")));
        assert_eq!(paths(&ws), ["a.rs", "b.rs"]);
        assert!(ws.is_preview(0));
        assert!(!ws.is_preview(1));
        assert_eq!(ws.active_index(), Some(1));
    }

    #[test]
    fn open_pinned_promotes_an_existing_preview_tab_in_place() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(!ws.open_pinned(item("a.rs")), "already active: no content change");
        assert_eq!(paths(&ws), ["a.rs"]);
        assert!(!ws.is_preview(0), "double-open must pin the preview tab");
    }

    #[test]
    fn activate_tab_out_of_range_is_a_no_op() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(!ws.activate_tab(5));
        assert_eq!(ws.active_index(), Some(0));
    }
}
```

- [ ] **Step 3: Register the module**

In `src/lib.rs`, add `pub mod workspace;` next to the existing `pub mod` declarations. Create an empty `src/workspace/tab_bar.rs` containing only a module doc comment for now (it is implemented in Task 5):

```rust
//! Zed-styled tab bar rendering for the workspace pane. Implemented with the
//! tab-bar feature slice; state lives in the parent module.
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib workspace`
Expected: PASS (all 9 tests).

- [ ] **Step 5: Commit**

```bash
git add src/workspace/mod.rs src/workspace/tab_bar.rs src/lib.rs
git commit -m "feat(workspace): preview/pinned tab state machine"
```

---

### Task 3: Workspace state machine — close/promote/clear + test utilities

**Files:**
- Modify: `src/workspace/mod.rs` (tests + a `test_util` submodule; the implementation already landed in Task 2)

- [ ] **Step 1: Add the close/promote/clear unit tests**

Append inside `mod tests` in `src/workspace/mod.rs`:

```rust
    #[test]
    fn closing_the_active_tab_activates_the_right_neighbor() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        ws.activate_tab(1);
        assert!(ws.close_tab(1));
        assert_eq!(paths(&ws), ["a.rs", "c.rs"]);
        assert_eq!(ws.active_index(), Some(1), "right neighbor (c.rs) becomes active");
    }

    #[test]
    fn closing_the_last_tab_in_the_strip_activates_the_left_neighbor() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        assert!(ws.close_tab(1));
        assert_eq!(ws.active_index(), Some(0));
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_item() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        assert!(!ws.close_tab(0), "active content unchanged");
        assert_eq!(ws.active_index(), Some(0));
        assert_eq!(ws.active_item().unwrap().path(), "b.rs");
    }

    #[test]
    fn closing_the_only_tab_empties_the_workspace() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(ws.close_tab(0));
        assert!(ws.tabs().is_empty());
        assert_eq!(ws.active_index(), None);
        assert!(ws.active_item().is_none());
    }

    #[test]
    fn closing_the_preview_tab_clears_the_preview_slot() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.close_tab(1);
        ws.open_preview(item("c.rs"));
        assert_eq!(paths(&ws), ["a.rs", "c.rs"], "a fresh preview tab appends");
        assert!(ws.is_preview(1));
    }

    #[test]
    fn closing_a_tab_left_of_the_preview_shifts_the_preview_index() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.close_tab(0);
        assert!(ws.is_preview(0), "preview index follows the shifted tab");
        assert_eq!(ws.active_item().unwrap().path(), "b.rs");
    }

    #[test]
    fn promote_tab_pins_only_the_preview_tab() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        ws.promote_tab(0);
        assert!(!ws.is_preview(0));
        ws.open_preview(item("b.rs"));
        assert_eq!(paths(&ws), ["a.rs", "b.rs"], "promoted tab no longer absorbs previews");
    }

    #[test]
    fn clear_closes_everything() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.clear();
        assert!(ws.tabs().is_empty());
        assert_eq!(ws.active_index(), None);
        ws.open_preview(item("c.rs"));
        assert!(ws.is_preview(0));
    }

    #[test]
    fn close_tab_out_of_range_is_a_no_op() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(!ws.close_tab(7));
        assert_eq!(paths(&ws), ["a.rs"]);
    }
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib workspace`
Expected: PASS (18 tests). These exercise code written in Task 2; any failure is a bug in the state machine — fix the implementation, not the test.

- [ ] **Step 3: Add the shared double-click test helper**

gpui's public `simulate_click` always sends `click_count: 1`, so view tests need a raw-dispatch helper. Append to `src/workspace/mod.rs` (after `mod tests`):

```rust
#[cfg(test)]
pub(crate) mod test_util {
    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, PlatformInput, Point,
        Render, TestAppContext, WindowHandle,
    };

    /// Dispatch a platform-faithful double-click at `position`: a count-1
    /// down/up pair followed by a count-2 down/up pair. The public
    /// `VisualTestContext::simulate_click` helper hardcodes `click_count: 1`,
    /// so this goes through `Window::dispatch_event` directly.
    pub(crate) fn simulate_double_click<V: Render>(
        window: &WindowHandle<V>,
        cx: &mut TestAppContext,
        position: Point<Pixels>,
    ) {
        window
            .update(cx, |_view, window, cx| {
                for click_count in [1, 2] {
                    window.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            position,
                            button: MouseButton::Left,
                            modifiers: Modifiers::none(),
                            click_count,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            position,
                            button: MouseButton::Left,
                            modifiers: Modifiers::none(),
                            click_count,
                        }),
                        cx,
                    );
                }
            })
            .expect("dispatch double click");
        cx.run_until_parked();
    }
}
```

Note: `window.dispatch_event(event, cx)` compiles because `Context<V>` derefs to `gpui::App`. If the borrow checker objects, call it as `window.dispatch_event(event, cx.deref_mut())`.

- [ ] **Step 4: Build the test target**

Run: `cargo test --lib workspace`
Expected: PASS, no warnings about the new helper (it is referenced from app tests in Task 4; `pub(crate)` in a `#[cfg(test)]` module does not trip dead-code in test builds — if it does, the Task 4 usage lands within the same `bin/check` run at the end of Task 4; run `cargo build --tests` here only to confirm it compiles).

- [ ] **Step 5: Commit**

```bash
git add src/workspace/mod.rs
git commit -m "feat(workspace): close/promote/clear semantics and double-click test helper"
```

---

### Task 4: App integration — workspace replaces `selected_changed_file_path`

**Files:**
- Modify: `src/app.rs` (struct fields ~`63-89`, constructor init `~326-346`, `open_repository_at` reset `~436`, `open_changeset` `~571-590`, `close_changeset` `~592-596`, `select_changed_file` `~603-607`, row click listeners `~1430`, `~1523`, `~1605`, `render_changeset_screen` `~1034-1077`, `render_file_detail` callers, plus the `#[cfg(test)]` module)
- Modify: `tests/smoke.rs` (compile fix only here; golden-path extension is Task 6)

Line numbers are pre-change anchors; locate by searching the named symbols.

- [ ] **Step 1: Inventory every use of the old field**

Search `src/app.rs` and `tests/` for `selected_changed_file_path` and `select_changed_file`. Bucket each hit:
- **Tree-row highlight** (computing a row's `selected` flag) → will read `file_tree_highlight_path`.
- **Detail-pane content** (what diff renders) → will read `workspace.active_item()`.
- **Lifecycle resets** (repo open, changeset open/close) → will call `workspace.clear()` + highlight reset.
- **Tests** → updated in Step 6.

- [ ] **Step 2: Swap the App fields**

In the `App` struct, replace:

```rust
    pub selected_changed_file_path: Option<String>,
```

with:

```rust
    /// Open diff tabs. Source of truth for what the detail area shows.
    pub workspace: crate::workspace::Workspace,
    /// Last file row the user clicked. Drives the tree highlight only; tab
    /// activation deliberately does not move it (spec: tree is click-driven).
    pub file_tree_highlight_path: Option<String>,
    /// Horizontal scroll for the tab strip.
    tab_bar_scroll: ScrollHandle,
```

In the constructor's `Self { ... }` literal (end of `new_with_picker_settings_and_store_path`), replace `selected_changed_file_path: None,` with:

```rust
    workspace: crate::workspace::Workspace::new(),
    file_tree_highlight_path: None,
    tab_bar_scroll: ScrollHandle::new(),
```

Add `use crate::workspace::{self, FileDiffItem};` is NOT needed if you use the `crate::workspace::` paths above; prefer adding `use crate::workspace::FileDiffItem;` to the existing import block and writing `crate::workspace::Workspace` at the two field sites.

- [ ] **Step 3: Replace `select_changed_file` with preview/pinned open methods**

Delete `fn select_changed_file` (src/app.rs:603-607) and add:

```rust
    fn open_file_preview(&mut self, path: String, cx: &mut Context<Self>) {
        self.file_tree_highlight_path = Some(path.clone());
        if self.workspace.open_preview(Box::new(FileDiffItem::new(path))) {
            self.file_diff_scroll.reset();
        }
        if let Some(index) = self.workspace.active_index() {
            self.tab_bar_scroll.scroll_to_item(index);
        }
        cx.notify();
    }

    fn open_file_pinned(&mut self, path: String, cx: &mut Context<Self>) {
        self.file_tree_highlight_path = Some(path.clone());
        if self.workspace.open_pinned(Box::new(FileDiffItem::new(path))) {
            self.file_diff_scroll.reset();
        }
        if let Some(index) = self.workspace.active_index() {
            self.tab_bar_scroll.scroll_to_item(index);
        }
        cx.notify();
    }

    pub(crate) fn activate_workspace_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.workspace.activate_tab(index) {
            self.file_diff_scroll.reset();
        }
        self.tab_bar_scroll.scroll_to_item(index);
        cx.notify();
    }

    pub(crate) fn promote_workspace_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        self.workspace.promote_tab(index);
        cx.notify();
    }

    pub(crate) fn close_workspace_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.workspace.close_tab(index) {
            self.file_diff_scroll.reset();
        }
        cx.notify();
    }
```

- [ ] **Step 4: Make the three file-row click sites click-count aware**

At each of the three render sites whose listener currently calls `app.select_changed_file(path.clone(), cx)` — the changed-file gutter cell (~1430), `render_changed_file_row` (~1523), and `render_unchanged_file_row` (~1605) — replace the listener with:

```rust
            .on_click(cx.listener(move |app, event: &ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    app.open_file_pinned(path.clone(), cx);
                } else {
                    app.open_file_preview(path.clone(), cx);
                }
            }))
```

`ClickEvent` is already imported in `src/app.rs`.

- [ ] **Step 5: Clear the workspace at lifecycle boundaries**

In `open_changeset` (~571-590): the block that conditionally preserved `selected_changed_file_path` (the `if !self.selected_changed_file_path...` statement) becomes an unconditional reset — the spec scopes tabs to one changeset:

```rust
                self.workspace.clear();
                self.file_tree_highlight_path = None;
                self.file_diff_scroll.reset();
```

In `close_changeset` (~592-596), add the same three lines before `cx.notify()`.

In `open_repository_at` (~436), replace `self.selected_changed_file_path = None;` (and its `file_diff_scroll.reset()` neighbor if adjacent) with the same three lines.

- [ ] **Step 6: Drive rendering from the workspace**

In `render_changeset_screen` (~1034), the `selected_path` binding currently filters `self.selected_changed_file_path` against the visible entries and feeds BOTH the tree and the detail pane. Split it:

```rust
                let highlight_path = self
                    .file_tree_highlight_path
                    .as_deref()
                    .filter(|path| entries.iter().any(|entry| entry.path() == *path));
                let active_path = self
                    .workspace
                    .active_item()
                    .map(|item| item.path().to_string());
```

Pass `highlight_path` wherever the file list previously received the selection (follow the existing plumbing into `render_file_list` / row `selected` flags — rename variables as needed), and change the detail panel child to a column that will hold the tab bar (Task 5) above the diff:

```rust
                        .child(
                            resizable_panel().child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .child(self.render_file_detail(
                                        repo,
                                        changeset,
                                        active_path.as_deref(),
                                    )),
                            ),
                        ),
```

`render_file_detail` itself is unchanged: `None` renders the existing `file-detail-empty` placeholder, a path not in the changeset renders read-only — both already match the spec.

- [ ] **Step 7: Update in-crate tests and the smoke compile**

In `src/app.rs` tests and `tests/smoke.rs`, mechanical rewrites:
- Assertions like `app.selected_changed_file_path == Some("x".into())` asserting the *open diff* become `assert_eq!(app.workspace.active_item().map(|item| item.path().to_string()), Some("x".to_string()))`.
- Assertions about the *tree highlight* (the `selected-changed-file-row-N` selector tests) keep working via the selector; where they read the field directly, use `app.file_tree_highlight_path`.
- Any direct call to `app.select_changed_file(path, cx)` becomes `app.open_file_preview(path, cx)`.
- `tests/smoke.rs:108-111` becomes the workspace assertion (smoke can read `app.workspace` because both field and accessors are pub).

- [ ] **Step 8: Run the full test suite**

Run: `cargo test`
Expected: PASS. Pay attention to tests that asserted selection persistence across changeset close/reopen — the old design preserved the selected file across reopen; the new spec clears tabs on changeset open/close. If such a test exists, INVERT its assertion to match the new contract (tabs cleared, placeholder shown) and rename it accordingly.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs tests/smoke.rs
git commit -m "feat(workspace): route file opening through preview/pinned tabs"
```

---

### Task 5: Tab bar rendering + view tests

**Files:**
- Modify: `src/workspace/tab_bar.rs` (full implementation + view tests)
- Modify: `src/app.rs` (`change_kind_text` visibility; mount the tab bar in `render_changeset_screen`)

- [ ] **Step 1: Expose the change-kind color helper**

In `src/app.rs`, change `fn change_kind_text(kind: repo::ChangeKind) -> gpui::Rgba` (~3897) to `pub(crate) fn change_kind_text(...)`.

- [ ] **Step 2: Write a failing view test**

Replace the placeholder content of `src/workspace/tab_bar.rs` with the test module first (implementation stub so it compiles):

```rust
//! Zed-styled tab bar for the workspace pane.
//!
//! One compact strip above the diff: hairline-separated tabs, active tab on
//! the editor background with a top accent line, preview titles in italics,
//! hover-revealed close buttons. Behavior contract lives in
//! `docs/specs/review/workflow.md`.

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, rgb, AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, ScrollHandle, StatefulInteractiveElement, Styled,
};
use gpui_component::Icon;

use super::Workspace;
use crate::app::{change_kind_text, App};
use crate::icons::LucideIcon;
use crate::repo;

pub fn render_tab_bar(
    workspace: &Workspace,
    changeset: &repo::ChangeSet,
    scroll: &ScrollHandle,
    cx: &mut Context<App>,
) -> AnyElement {
    let _ = (workspace, changeset, scroll, cx);
    div().into_any_element()
}
```

Append the test module:

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::workspace::test_util::simulate_double_click;
    use git2::{IndexAddOption, Oid, Repository, Signature};
    use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle};
    use std::fs;

    fn add_app_window(cx: &mut TestAppContext) -> WindowHandle<App> {
        cx.update(gpui_component::init);
        cx.add_window(App::new)
    }

    fn commit_all(repo: &Repository, message: &str, parent_shas: &[String]) -> String {
        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let sig = Signature::now("Greviewer Tests", "tests@greviewer.invalid")
            .expect("create signature");
        let parents: Vec<git2::Commit> = parent_shas
            .iter()
            .map(|sha| {
                repo.find_commit(Oid::from_str(sha).expect("parse oid"))
                    .expect("find parent")
            })
            .collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
            .expect("create commit");
        oid.to_string()
    }

    /// Two-commit repo whose head changes `alpha.txt` and `nested/beta.txt`.
    fn init_repo_with_two_changed_files() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        fs::create_dir_all(dir.path().join("nested")).expect("create nested dir");
        fs::write(dir.path().join("alpha.txt"), "alpha v1\n").expect("write alpha");
        fs::write(dir.path().join("nested/beta.txt"), "beta v1\n").expect("write beta");
        let first = commit_all(&repo, "Add files", &[]);
        fs::write(dir.path().join("alpha.txt"), "alpha v2\n").expect("update alpha");
        fs::write(dir.path().join("nested/beta.txt"), "beta v2\n").expect("update beta");
        let head = commit_all(&repo, "Update files", &[first]);
        drop(repo);
        (dir, head)
    }

    /// Open the fixture repo, select the head commit, and click into the
    /// changeset review screen. Returns the visual context for clicking.
    fn open_changeset(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        let (dir, head) = init_repo_with_two_changed_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);
        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(head, cx);
            })
            .expect("open repo and select commit");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());
        cx.run_until_parked();
        (dir, window, visual)
    }

    fn click_file_row(visual: &mut VisualTestContext, index: usize) {
        let bounds = visual
            .debug_bounds(match index {
                0 => "changed-file-row-0",
                1 => "changed-file-row-1",
                _ => panic!("unsupported row index"),
            })
            .or_else(|| {
                visual.debug_bounds(match index {
                    0 => "selected-changed-file-row-0",
                    1 => "selected-changed-file-row-1",
                    _ => unreachable!(),
                })
            })
            .expect("file row debug bounds");
        visual.simulate_click(bounds.center(), Modifiers::none());
    }

    #[gpui::test]
    async fn single_clicks_share_one_preview_tab(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        click_file_row(&mut visual, 0);
        visual
            .debug_bounds("workspace-tab-0")
            .expect("first tab renders");
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("workspace-tab-1").is_none(),
            "second single-click must reuse the preview tab, not add a tab"
        );
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs().len(), 1);
                assert!(app.workspace.is_preview(0));
                assert_eq!(
                    app.workspace.active_item().map(|item| item.path().to_string()),
                    Some("nested/beta.txt".to_string()),
                );
            })
            .expect("read workspace state");
    }

    #[gpui::test]
    async fn double_clicking_a_file_row_pins_the_tab(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("file row debug bounds");
        simulate_double_click(&window, cx, row_bounds.center());

        let mut visual = VisualTestContext::from_window(*window, cx);
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs().len(), 2, "pinned tab plus new preview");
                assert!(!app.workspace.is_preview(0), "double-clicked tab is pinned");
                assert!(app.workspace.is_preview(1));
            })
            .expect("read workspace state");
    }

    #[gpui::test]
    async fn clicking_a_tab_activates_it_and_double_click_promotes(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("file row debug bounds");
        simulate_double_click(&window, cx, row_bounds.center());
        let mut visual = VisualTestContext::from_window(*window, cx);
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Click the first (pinned) tab: it activates, tree highlight stays put.
        let tab0 = visual.debug_bounds("workspace-tab-0").expect("tab 0");
        visual.simulate_click(tab0.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.active_index(), Some(0));
                assert_eq!(
                    app.file_tree_highlight_path,
                    Some("nested/beta.txt".to_string()),
                    "tab activation must not move the tree highlight"
                );
            })
            .expect("read activation state");

        // Double-click the preview tab: it pins in place.
        let tab1 = visual.debug_bounds("workspace-tab-1").expect("tab 1");
        simulate_double_click(&window, cx, tab1.center());
        window
            .read_with(cx, |app, _cx| {
                assert!(!app.workspace.is_preview(1), "double-clicked tab is promoted");
            })
            .expect("read promotion state");
    }

    #[gpui::test]
    async fn close_button_and_middle_click_close_tabs(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("file row debug bounds");
        simulate_double_click(&window, cx, row_bounds.center());
        let mut visual = VisualTestContext::from_window(*window, cx);
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Close the active (preview) tab via its close button.
        let close1 = visual
            .debug_bounds("workspace-tab-close-1")
            .expect("close button on active tab");
        visual.simulate_click(close1.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs().len(), 1);
                assert_eq!(
                    app.workspace.active_item().map(|item| item.path().to_string()),
                    Some("alpha.txt".to_string()),
                    "left neighbor becomes active"
                );
            })
            .expect("read state after close-button close");

        // Middle-click closes the remaining tab; the placeholder returns.
        let tab0 = visual.debug_bounds("workspace-tab-0").expect("tab 0");
        visual.simulate_mouse_down(tab0.center(), MouseButton::Middle, Modifiers::none());
        visual.simulate_mouse_up(tab0.center(), MouseButton::Middle, Modifiers::none());
        cx.run_until_parked();

        assert!(visual.debug_bounds("workspace-tab-0").is_none(), "tab bar empty");
        visual
            .debug_bounds("file-detail-empty")
            .expect("placeholder returns when every tab is closed");
        window
            .read_with(cx, |app, _cx| assert!(app.workspace.tabs().is_empty()))
            .expect("read emptied workspace");
    }
}
```

Note on `click_file_row`: after a click, row 0's selector may flip to `selected-changed-file-row-0` (tree highlight), hence the fallback chain. `debug_bounds` returns `Option`, so `.or_else(...)` composes; check the actual return type — if it is `Option<Bounds<Pixels>>` this code is correct as written.

- [ ] **Step 3: Run the new tests to verify they fail**

Run: `cargo test --lib workspace::tab_bar`
Expected: FAIL — `debug_bounds("workspace-tab-0")` panics on `expect` because the stub renders nothing.

- [ ] **Step 4: Implement `render_tab_bar`**

Replace the stub body in `src/workspace/tab_bar.rs`:

```rust
const TAB_BAR_HEIGHT: f32 = 32.;
const TAB_TEXT_SIZE: f32 = 13.;
const TAB_DIR_HINT_TEXT_SIZE: f32 = 10.;
const TAB_FONT_FAMILY: &str = "BerkeleyMono Nerd Font";
const TAB_BAR_BG: u32 = 0x111111;
const TAB_ACTIVE_BG: u32 = 0x171717;
const TAB_BORDER: u32 = 0x2a2a2a;
const TAB_ACCENT: u32 = 0x7da4ff;
const TAB_MUTED_TEXT: u32 = 0x8a8a8a;
const TAB_DEFAULT_TEXT: u32 = 0xe6eef0;
const TAB_CLOSE_HIT_SIZE: f32 = 16.;
const TAB_CLOSE_ICON_SIZE: f32 = 12.;
const TAB_INACTIVE_OPACITY: f32 = 0.75;

/// The innermost directory name, shown to disambiguate duplicate file names.
fn parent_directory_hint(path: &str) -> Option<String> {
    let (dir, _file) = path.rsplit_once('/')?;
    Some(dir.rsplit('/').next().unwrap_or(dir).to_string())
}

pub fn render_tab_bar(
    workspace: &Workspace,
    changeset: &repo::ChangeSet,
    scroll: &ScrollHandle,
    cx: &mut Context<App>,
) -> AnyElement {
    if workspace.tabs().is_empty() {
        return div().into_any_element();
    }

    let mut bar = div()
        .id("workspace-tab-bar")
        .debug_selector(|| "workspace-tab-bar".into())
        .flex()
        .items_center()
        .w_full()
        .h(px(TAB_BAR_HEIGHT))
        .flex_none()
        .bg(rgb(TAB_BAR_BG))
        .border_b_1()
        .border_color(rgb(TAB_BORDER))
        .overflow_x_scroll()
        .track_scroll(scroll);

    for (index, item) in workspace.tabs().iter().enumerate() {
        let active = workspace.active_index() == Some(index);
        let preview = workspace.is_preview(index);
        let title = item.tab_title().to_string();
        let duplicate_title = workspace
            .tabs()
            .iter()
            .enumerate()
            .any(|(other, tab)| other != index && tab.tab_title() == title);
        let parent_hint = duplicate_title
            .then(|| parent_directory_hint(item.path()))
            .flatten();
        let title_color = changeset
            .files
            .iter()
            .find(|file| file.path == item.path())
            .map(|file| change_kind_text(file.kind))
            .unwrap_or(rgb(TAB_DEFAULT_TEXT));
        let tab_selector = format!("workspace-tab-{index}");
        let close_selector = format!("workspace-tab-close-{index}");
        let group_name = format!("workspace-tab-{index}");

        let close_button = div()
            .id(("workspace-tab-close", index))
            .debug_selector(move || close_selector.clone())
            .flex()
            .items_center()
            .justify_center()
            .w(px(TAB_CLOSE_HIT_SIZE))
            .h(px(TAB_CLOSE_HIT_SIZE))
            .rounded(px(2.))
            .when(!active, |button| {
                button
                    .opacity(0.)
                    .group_hover(group_name.clone(), |button| button.opacity(1.))
            })
            .hover(|button| button.bg(rgb(TAB_BORDER)))
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                app.close_workspace_tab(index, cx);
            }))
            .child(
                Icon::new(LucideIcon::X)
                    .size(px(TAB_CLOSE_ICON_SIZE))
                    .text_color(rgb(TAB_MUTED_TEXT)),
            );

        let tab = div()
            .id(("workspace-tab", index))
            .debug_selector(move || tab_selector.clone())
            .group(group_name)
            .relative()
            .flex()
            .items_center()
            .gap_1()
            .h_full()
            .px_3()
            .flex_none()
            .cursor_pointer()
            .border_r_1()
            .border_color(rgb(TAB_BORDER))
            .when(active, |tab| {
                tab.bg(rgb(TAB_ACTIVE_BG)).child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(2.))
                        .bg(rgb(TAB_ACCENT)),
                )
            })
            .when(!active, |tab| tab.opacity(TAB_INACTIVE_OPACITY))
            .on_click(cx.listener(move |app, event: &ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    app.promote_workspace_tab(index, cx);
                } else {
                    app.activate_workspace_tab(index, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |app, _event: &MouseDownEvent, _window, cx| {
                    app.close_workspace_tab(index, cx);
                }),
            )
            .child(
                div()
                    .text_size(px(TAB_TEXT_SIZE))
                    .font_family(TAB_FONT_FAMILY)
                    .text_color(title_color)
                    .whitespace_nowrap()
                    .when(preview, |label| label.italic())
                    .child(title),
            )
            .when_some(parent_hint, |tab, hint| {
                tab.child(
                    div()
                        .text_size(px(TAB_DIR_HINT_TEXT_SIZE))
                        .font_family(TAB_FONT_FAMILY)
                        .text_color(rgb(TAB_MUTED_TEXT))
                        .whitespace_nowrap()
                        .child(hint),
                )
            })
            .child(close_button);

        bar = bar.child(tab);
    }

    bar.into_any_element()
}
```

Implementation notes:
- If `.italic()` does not exist on `Div` in gpui 0.2.2, use `.font_style(gpui::FontStyle::Italic)`; check `gpui::StyledTypography`/`Styled` for the available method and use whichever compiles.
- If `.group_hover` with per-tab unique names misbehaves, fall back to a shared `"workspace-tab"` group name; gpui scopes `group_hover` to the nearest enclosing group.
- An element with `opacity(0.)` is still laid out and hit-testable, so `debug_bounds` and `simulate_click` work on hidden close buttons — this is why visibility uses opacity rather than conditional rendering.

- [ ] **Step 5: Mount the tab bar in the changeset screen**

In `src/app.rs` `render_changeset_screen`, insert the tab bar above the detail child added in Task 4 Step 6:

```rust
                                    .child(crate::workspace::tab_bar::render_tab_bar(
                                        &self.workspace,
                                        changeset,
                                        &self.tab_bar_scroll,
                                        cx,
                                    ))
```

(The column becomes: tab bar child, then `render_file_detail` child.)

- [ ] **Step 6: Run the view tests**

Run: `cargo test --lib workspace::tab_bar`
Expected: PASS (4 tests).

- [ ] **Step 7: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/workspace/tab_bar.rs src/app.rs
git commit -m "feat(workspace): zed-styled tab bar with preview, pin, and close affordances"
```

---

### Task 6: Extend the smoke test golden path

**Files:**
- Modify: `tests/smoke.rs`

- [ ] **Step 1: Extend `boots_open_repo_renders_head_info`**

After the existing `changed-file-row-0` click and diff assertions (lines ~78-101), the Task 4 compile fix already swapped the state assertion. Now add the tab assertions to the golden path. After the `file-diff-row-added` assertion, insert:

```rust
    visual
        .debug_bounds("workspace-tab-0")
        .expect("opening a file shows its tab in the tab bar");
```

And extend the final `read_with` block (which now asserts via `app.workspace`) with the preview/pin transition:

```rust
    window
        .read_with(cx, |app, _cx| {
            assert!(app.workspace.is_preview(0), "single-click opens a preview tab");
        })
        .expect("read preview state");

    // Double-clicking the same row pins the tab.
    let row_bounds = visual
        .debug_bounds("selected-changed-file-row-0")
        .expect("selected changed file row debug bounds");
    greviewer_double_click(&window, cx, row_bounds.center());

    window
        .read_with(cx, |app, _cx| {
            assert_eq!(app.workspace.tabs().len(), 1);
            assert!(!app.workspace.is_preview(0), "double-click pins the tab");
        })
        .expect("read pinned state");
```

`crate::workspace::test_util` is `#[cfg(test)]`-gated inside the lib and NOT visible to integration tests, so add a local copy of the double-click helper to `tests/common/mod.rs` (or directly in `tests/smoke.rs`) named `greviewer_double_click`, with the same body as `workspace::test_util::simulate_double_click` (Task 3 Step 3) but `pub` and importing from `gpui`. Generic over `V: Render` exactly as before.

- [ ] **Step 2: Run the smoke test**

Run: `cargo test --test smoke`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add tests/smoke.rs tests/common
git commit -m "test(smoke): extend golden path through preview and pinned tabs"
```

---

### Task 7: Update the review workflow spec

**Files:**
- Modify: `docs/specs/review/workflow.md`

- [ ] **Step 1: Read the spec-voice rules**

Read `docs/specs/README.md` and skim the existing sections of `docs/specs/review/workflow.md` for voice (PM-voice, normative outcomes, no implementation detail).

- [ ] **Step 2: Update the file-selection touchpoints**

In "Reviewing the change set" → Observable outcomes, change the line `- Selecting a file opens it for inspection.` to:

```markdown
- Selecting a file opens it for inspection in a tab above the diff area (see "Holding files open in tabs").
```

- [ ] **Step 3: Add the tabs section**

Insert a new section between "Seeing all files for context" and "Inspecting a file's diff":

```markdown
## Holding files open in tabs

Opened files live in a row of tabs above the diff area. A single click on a file opens it in the preview tab — a holding slot that subsequent single clicks reuse, so casual browsing never piles up tabs. At most one preview tab exists at a time, and its title renders in italics to signal that the next single click will replace it. Opening a file deliberately — double-clicking it in the tree, or double-clicking the preview tab itself — pins the tab; a pinned tab keeps its file until the user closes it.

**Triggering conditions**

- The user single-clicks or double-clicks a file row while a changeset is open, or clicks, double-clicks, or middle-clicks a tab.

**Observable outcomes**

- Single-clicking a file shows its diff in the preview tab, creating that tab when none exists and otherwise replacing its content in place; the tab keeps its position in the row.
- Double-clicking a file pins its tab. Double-clicking the preview tab pins it without changing its content.
- Opening a file that is already open activates the existing tab; a file is never open in two tabs at once, and opening a pinned file never demotes it to preview.
- A tab's title is the file's name, tinted with the file's change-kind color; files opened from the all-files view that are not part of the change set use the default text color. When two open tabs share a file name, each also shows its parent folder name.
- Exactly one tab is active; it is visually distinct from the other tabs, and the diff area always shows the active tab's file. Clicking a tab activates it.
- Every tab offers a close control, revealed on hover and always present on the active tab; middle-clicking a tab also closes it. Closing the active tab activates its right neighbor, or its left neighbor at the end of the row.
- When more tabs are open than fit the width, the tab row scrolls horizontally and activating a tab brings it into view.
- Closing every tab returns the diff area to the select-a-file placeholder.
- Leaving the changeset — closing it or opening a different one — closes all tabs.
- The file tree's highlighted row follows the user's clicks in the tree; activating a different tab does not move the tree highlight.

**Edge cases**

- Closing the preview tab removes it; the next single click opens a fresh preview tab at the end of the row.
- Re-opening the changeset after leaving it starts with no tabs open, even if files were open when it was left.
```

- [ ] **Step 4: Reconcile "Inspecting a file's diff"**

Its triggering condition `The user selects a changed file in the change set.` still reads correctly (selection now opens a tab); no change needed unless wording conflicts after Step 2 — re-read the section and adjust only if a statement now contradicts the tabs section.

- [ ] **Step 5: Commit**

```bash
git add docs/specs/review/workflow.md
git commit -m "docs(spec): tabbed file opening in the review workflow"
```

---

### Task 8: Final verification

- [ ] **Step 1: Run the full gate**

Run: `bin/check`
Expected: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean, all tests pass. Fix anything it flags (no `#[allow]` without user approval).

- [ ] **Step 2: Review the diff against the spec**

Re-read `docs/superpowers/specs/2026-06-09-diff-workspace-tabs-design.md` sections "Tab Semantics", "Visual Design" and confirm each slice-1 behavior maps to landed code/tests. Splits, drag-and-drop, persistence, and keyboard bindings are explicitly later slices — they must NOT appear in this change.

- [ ] **Step 3: Commit any verification fixes**

```bash
git add -A ':!docs/superpowers'
git commit -m "fix(workspace): address bin/check findings"
```

(Skip if there were no findings.)
