# Diff Workspace Tabs — Slice 2 Implementation Plan (Splits)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Multiple panes: a ratio-weighted PaneGroup layout tree, split-right/split-down controls in each tab bar's corner, one active pane (dimmed inactive tab bars, tree clicks open in the active pane), close-pane collapsing, draggable dividers, and keyboard bindings (Cmd+W, Ctrl+Tab/Ctrl+Shift+Tab, Cmd+K arrow, Cmd+K W).

**Architecture:** `src/workspace/mod.rs` grows from one `Pane` to `panes: Vec<(PaneId, Pane)>` + a `PaneGroup` tree (axis nodes with normalized ratio weights, pane leaves) + `active_pane`. All transitions stay pure and gpui-free. A new `src/workspace/pane_grid.rs` renders the tree recursively; `tab_bar.rs` becomes pane-scoped and gains corner split controls. `App` replaces its single `tab_bar_scroll`/`file_diff_scroll` with a per-pane scroll map. Spec: `docs/superpowers/specs/2026-06-09-diff-workspace-tabs-design.md` (this plan implements slice 2 only).

**Tech Stack:** Rust, gpui 0.2.2, gpui-component 0.5. Verification: `bin/check`.

---

## Verified API facts (do not re-derive)

- **Deviation from the design's "reuse resizable-panel machinery where practical":** gpui-component's `ResizableState` has **no public write API** (only `sizes()`); restored/initial ratios cannot be injected, and `sync_panels_count` seeds new panels at a fixed 100px. That makes it impractical for ratio-weighted splits and for slice 4's persistence. Dividers are instead implemented with gpui's own drag primitives, and ratios live in our `PaneGroup` — the very value slice 4 persists. The file-tree/detail split keeps using `h_resizable`.
- `on_drag(value: T, constructor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W>) ` is on `StatefulInteractiveElement` (element needs `.id(...)`). The constructor returns the drag-preview entity (`W: Render`).
- `on_drag_move::<T>(listener: impl Fn(&DragMoveEvent<T>, &mut Window, &mut App))` is on `InteractiveElement`; fires for every mouse move while a drag of value type `T` is active, regardless of pointer position. `event.bounds` is the bounds of the element the listener is attached to; `event.event.position` is the mouse position; `event.drag(cx)` returns `&T` (immutable borrow of the gpui app — copy fields out before calling `&mut` methods).
- A drag activates after mouse-down + a >2px move with the button held; in tests, `visual.simulate_mouse_down(...)`, then `visual.simulate_mouse_move(pos, MouseButton::Left, Modifiers::none())` (signature: `(Point<Pixels>, impl Into<Option<MouseButton>>, Modifiers)`), then `simulate_mouse_up`.
- `KeyBinding::new("cmd-k right", Action, None)` — multi-keystroke sequences are space-separated (`split_whitespace` in `KeyBinding::load`). `"ctrl-tab"`, `"ctrl-shift-tab"`, `"cmd-w"`, `"cmd-k w"` all parse. Context `None` = matches everywhere.
- `VisualTestContext::simulate_keystrokes("cmd-k right")` dispatches through the keymap to the focused element and runs until parked. App's root div has `.track_focus(&self.focus_handle)` + `.on_action(...)` chain (`src/app.rs:4505-4537`), and the constructor focuses the handle.
- `Pixels / Pixels -> f32` (used for fraction math). `gpui::relative(fraction: f32) -> DefiniteLength`; `.flex_basis(impl Into<Length>)` accepts it. Ratio-weighted flex children: `.flex_basis(relative(ratio))` with default `flex_shrink` absorbing the few px the 4px dividers consume.
- `Styled` has `.flex_row()`, `.flex_col()`, `.cursor_col_resize()`, `.cursor_row_resize()`, `.opacity(f32)`. `FluentBuilder` has `.map(...)`, `.when(...)`, `.when_some(...)`.
- `ElementId: From<SharedString>` — use `SharedString::from(format!(...))` for dynamic ids; tuple ids `(&'static str, usize)` also work.
- `ScrollHandle` is `Clone` and clones **share** the underlying state — cloning out of a `RefCell` map hands the same scroll position to render code.
- gpui never clears `debug_bounds` once painted → no absence assertions on once-painted selectors (slice-1 rule). Double-click helper: `workspace::test_util::simulate_double_click(&mut visual, pos)`.
- Existing key-dispatch test pattern: search `src/app.rs` tests for the `QuitRequested` test to see how bindings are registered in tests (it calls `crate::app::menu` helpers); replicate for the new bindings.
- Slice-1 anchors in `src/app.rs` (post-change line numbers drift; search symbols): `App` struct fields `~65-102`, constructor literal `~340-365`, `apply_open_repository` `~438`, `open_changeset` `~637`, `close_changeset` `~672`, `open_file_preview`/`open_file_pinned`/`activate_workspace_tab`/`promote_workspace_tab`/`close_workspace_tab` `~687-736`, `render_changeset_screen` `~1324`, `render_file_detail` `~1946`, `render_changed_file_detail` (uses `&self.file_diff_scroll`) `~1994`, `render_read_only_file_detail` `~2056`, `actions!` `37-45`, `impl Render` `~4505`.

---

### Task 1: Vendor the Lucide split icons

**Files:**
- Create: `assets/icons/columns-2.svg`, `assets/icons/rows-2.svg`
- Modify: `src/icons.rs`

- [ ] **Step 1: Add the SVG assets** (Lucide, ISC license, same formatting as existing vendored icons)

`assets/icons/columns-2.svg`:

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
  <rect width="18" height="18" x="3" y="3" rx="2" />
  <path d="M12 3v18" />
</svg>
```

`assets/icons/rows-2.svg`:

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
  <rect width="18" height="18" x="3" y="3" rx="2" />
  <path d="M3 12h18" />
</svg>
```

- [ ] **Step 2: Register the variants**

In `src/icons.rs` add `Columns2` and `Rows2` to the `LucideIcon` enum in alphabetical position, to the `ALL` array, and to `path()`:

```rust
            LucideIcon::Columns2 => "icons/columns-2.svg",
            LucideIcon::Rows2 => "icons/rows-2.svg",
```

- [ ] **Step 3: Run the icon asset test**

Run: `cargo test --lib icons`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add assets/icons/columns-2.svg assets/icons/rows-2.svg src/icons.rs
git commit -m "feat(icons): vendor lucide split icons for pane controls"
```

---

### Task 2: Workspace state machine — pane tree, splits, collapse, resize, cycling

**Files:**
- Modify: `src/workspace/mod.rs`

This task reworks workspace internals to multiple panes while keeping the slice-1 no-pane methods as thin active-pane delegates so `app.rs`/`tab_bar.rs` keep compiling. The delegates are removed in Task 4 when every call site goes pane-scoped.

- [ ] **Step 1: Add the layout types and rework `Workspace`**

In `src/workspace/mod.rs`, update the module doc (`slice 1` → `slices 1-2`, mention the pane tree), add `pub mod pane_grid;` next to `pub mod tab_bar;`, and replace the `Pane`/`Workspace` definitions (keep `WorkspaceItem` and `FileDiffItem` unchanged):

```rust
/// Identifies one pane for the lifetime of a workspace.
pub type PaneId = usize;

/// Smallest share of an axis a pane can be resized down to.
pub const MIN_PANE_RATIO: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    /// Children sit side by side.
    Horizontal,
    /// Children stack top to bottom.
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Left,
    Right,
    Up,
    Down,
}

impl SplitDirection {
    pub fn axis(self) -> SplitAxis {
        match self {
            SplitDirection::Left | SplitDirection::Right => SplitAxis::Horizontal,
            SplitDirection::Up | SplitDirection::Down => SplitAxis::Vertical,
        }
    }

    /// Whether the new pane lands after the source pane along the axis.
    fn inserts_after(self) -> bool {
        matches!(self, SplitDirection::Right | SplitDirection::Down)
    }
}

/// The workspace layout: axis nodes with ratio-weighted children, pane leaves.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneGroup {
    Pane(PaneId),
    Axis(AxisNode),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisNode {
    /// Stable identity for divider-drag routing; allocated from the same
    /// counter as pane ids.
    pub id: usize,
    pub axis: SplitAxis,
    /// One fraction per child, kept normalized to sum to 1.
    pub ratios: Vec<f32>,
    pub children: Vec<PaneGroup>,
}

/// One tab strip plus its contents.
struct Pane {
    tabs: Vec<Box<dyn WorkspaceItem>>,
    active: Option<usize>,
    /// Index of the preview tab, if one exists. At most one per pane.
    preview: Option<usize>,
}

impl Pane {
    fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            preview: None,
        }
    }
}

pub struct Workspace {
    panes: Vec<(PaneId, Pane)>,
    layout: PaneGroup,
    active_pane: PaneId,
    /// Shared id counter for panes and axis nodes.
    next_id: usize,
}
```

- [ ] **Step 2: Rework the `Workspace` impl**

Replace the whole `impl Workspace` block. Existing tab semantics move to pane-scoped methods; the old no-pane signatures become active-pane delegates (deleted in Task 4):

```rust
impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            panes: vec![(0, Pane::new())],
            layout: PaneGroup::Pane(0),
            active_pane: 0,
            next_id: 1,
        }
    }

    fn pane(&self, pane: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|(id, _)| *id == pane).map(|(_, p)| p)
    }

    fn pane_mut(&mut self, pane: PaneId) -> Option<&mut Pane> {
        self.panes
            .iter_mut()
            .find(|(id, _)| *id == pane)
            .map(|(_, p)| p)
    }

    pub fn layout(&self) -> &PaneGroup {
        &self.layout
    }

    pub fn active_pane(&self) -> PaneId {
        self.active_pane
    }

    /// Pane ids in layout (left-to-right, top-to-bottom) order.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        Self::collect_pane_ids(&self.layout, &mut ids);
        ids
    }

    fn collect_pane_ids(node: &PaneGroup, out: &mut Vec<PaneId>) {
        match node {
            PaneGroup::Pane(id) => out.push(*id),
            PaneGroup::Axis(axis) => {
                for child in &axis.children {
                    Self::collect_pane_ids(child, out);
                }
            }
        }
    }

    /// Returns true when the active pane changed.
    pub fn activate_pane(&mut self, pane: PaneId) -> bool {
        if self.active_pane == pane || self.pane(pane).is_none() {
            return false;
        }
        self.active_pane = pane;
        true
    }

    pub fn tabs(&self, pane: PaneId) -> &[Box<dyn WorkspaceItem>] {
        self.pane(pane).map(|p| p.tabs.as_slice()).unwrap_or(&[])
    }

    pub fn active_index(&self, pane: PaneId) -> Option<usize> {
        self.pane(pane)?.active
    }

    pub fn active_item(&self, pane: PaneId) -> Option<&dyn WorkspaceItem> {
        let pane = self.pane(pane)?;
        pane.active
            .and_then(|index| pane.tabs.get(index))
            .map(|item| item.as_ref())
    }

    pub fn is_preview(&self, pane: PaneId, index: usize) -> bool {
        self.pane(pane).is_some_and(|p| p.preview == Some(index))
    }

    /// Open `item` in the active pane's preview slot. Returns true when that
    /// pane's shown content changed (callers reset diff scroll on true).
    pub fn open_preview(&mut self, item: Box<dyn WorkspaceItem>) -> bool {
        let pane_id = self.active_pane;
        let Some(pane) = self.pane_mut(pane_id) else {
            return false;
        };
        if let Some(index) = pane.tabs.iter().position(|tab| tab.key() == item.key()) {
            return self.activate_tab(pane_id, index);
        }
        match pane.preview {
            Some(index) => {
                pane.tabs[index] = item;
                pane.active = Some(index);
                true
            }
            None => {
                pane.tabs.push(item);
                let index = pane.tabs.len() - 1;
                pane.preview = Some(index);
                pane.active = Some(index);
                true
            }
        }
    }

    /// Open `item` pinned in the active pane. An existing tab for the same key
    /// is activated and promoted out of preview. Returns true when the pane's
    /// shown content changed.
    pub fn open_pinned(&mut self, item: Box<dyn WorkspaceItem>) -> bool {
        let pane_id = self.active_pane;
        let Some(pane) = self.pane_mut(pane_id) else {
            return false;
        };
        if let Some(index) = pane.tabs.iter().position(|tab| tab.key() == item.key()) {
            if pane.preview == Some(index) {
                pane.preview = None;
            }
            return self.activate_tab(pane_id, index);
        }
        pane.tabs.push(item);
        pane.active = Some(pane.tabs.len() - 1);
        true
    }

    /// Returns true when the pane's shown content changed.
    pub fn activate_tab(&mut self, pane: PaneId, index: usize) -> bool {
        let Some(pane) = self.pane_mut(pane) else {
            return false;
        };
        if index >= pane.tabs.len() || pane.active == Some(index) {
            return false;
        }
        pane.active = Some(index);
        true
    }

    /// Promote the tab at `index` out of preview, if it is the preview tab.
    pub fn promote_tab(&mut self, pane: PaneId, index: usize) {
        if let Some(pane) = self.pane_mut(pane) {
            if pane.preview == Some(index) {
                pane.preview = None;
            }
        }
    }

    /// Close the tab at `index` in `pane`. The right neighbor (or left, at the
    /// end of the strip) becomes active. Returns true when the pane's shown
    /// content changed.
    pub fn close_tab(&mut self, pane: PaneId, index: usize) -> bool {
        let Some(pane) = self.pane_mut(pane) else {
            return false;
        };
        if index >= pane.tabs.len() {
            return false;
        }
        pane.tabs.remove(index);
        match pane.preview {
            Some(preview) if preview == index => pane.preview = None,
            Some(preview) if preview > index => pane.preview = Some(preview - 1),
            _ => {}
        }
        let previous_active = pane.active;
        pane.active = if pane.tabs.is_empty() {
            None
        } else {
            match previous_active {
                Some(active) if active == index => Some(index.min(pane.tabs.len() - 1)),
                Some(active) if active > index => Some(active - 1),
                other => other,
            }
        };
        previous_active == Some(index)
    }

    /// Cycle the active pane's active tab forward or backward, wrapping.
    /// Returns true when the pane's shown content changed.
    pub fn activate_next_tab(&mut self) -> bool {
        self.cycle_tab(1)
    }

    pub fn activate_previous_tab(&mut self) -> bool {
        self.cycle_tab(-1)
    }

    fn cycle_tab(&mut self, step: isize) -> bool {
        let Some(pane) = self.pane_mut(self.active_pane) else {
            return false;
        };
        let len = pane.tabs.len() as isize;
        let Some(active) = pane.active else {
            return false;
        };
        if len < 2 {
            return false;
        }
        pane.active = Some((active as isize + step).rem_euclid(len) as usize);
        true
    }

    /// Split `pane`, inserting a new empty pane adjacent to it in `direction`.
    /// The new pane becomes active. Returns its id, or None when `pane` does
    /// not exist.
    pub fn split(&mut self, pane: PaneId, direction: SplitDirection) -> Option<PaneId> {
        self.pane(pane)?;
        let new_pane = self.next_id;
        self.next_id += 1;
        if !Self::split_in_node(&mut self.layout, pane, direction, new_pane, &mut self.next_id) {
            self.next_id -= 1;
            return None;
        }
        self.panes.push((new_pane, Pane::new()));
        self.active_pane = new_pane;
        Some(new_pane)
    }

    fn split_in_node(
        node: &mut PaneGroup,
        target: PaneId,
        direction: SplitDirection,
        new_pane: PaneId,
        next_id: &mut usize,
    ) -> bool {
        match node {
            PaneGroup::Pane(id) if *id == target => {
                let (first, second) = if direction.inserts_after() {
                    (PaneGroup::Pane(target), PaneGroup::Pane(new_pane))
                } else {
                    (PaneGroup::Pane(new_pane), PaneGroup::Pane(target))
                };
                let id = *next_id;
                *next_id += 1;
                *node = PaneGroup::Axis(AxisNode {
                    id,
                    axis: direction.axis(),
                    ratios: vec![0.5, 0.5],
                    children: vec![first, second],
                });
                true
            }
            PaneGroup::Pane(_) => false,
            PaneGroup::Axis(axis) => {
                // Splitting along the parent's own axis inserts a sibling that
                // takes half the source pane's share instead of nesting.
                if axis.axis == direction.axis() {
                    let target_position = axis.children.iter().position(
                        |child| matches!(child, PaneGroup::Pane(id) if *id == target),
                    );
                    if let Some(position) = target_position {
                        let half = axis.ratios[position] / 2.;
                        axis.ratios[position] = half;
                        let insert_at = if direction.inserts_after() {
                            position + 1
                        } else {
                            position
                        };
                        axis.children.insert(insert_at, PaneGroup::Pane(new_pane));
                        axis.ratios.insert(insert_at, half);
                        return true;
                    }
                }
                axis.children.iter_mut().any(|child| {
                    Self::split_in_node(child, target, direction, new_pane, next_id)
                })
            }
        }
    }

    /// Close `pane`, collapsing its slot and returning its space to siblings.
    /// Refused (returns false) for the last remaining pane or an unknown id.
    pub fn close_pane(&mut self, pane: PaneId) -> bool {
        let order = self.pane_ids();
        let Some(closed_position) = order.iter().position(|id| *id == pane) else {
            return false;
        };
        if order.len() <= 1 {
            return false;
        }
        Self::remove_pane_from_node(&mut self.layout, pane);
        Self::collapse_single_child_axes(&mut self.layout);
        self.panes.retain(|(id, _)| *id != pane);
        if self.active_pane == pane {
            let order = self.pane_ids();
            self.active_pane = order[closed_position.min(order.len() - 1)];
        }
        true
    }

    fn remove_pane_from_node(node: &mut PaneGroup, target: PaneId) -> bool {
        let PaneGroup::Axis(axis) = node else {
            return false;
        };
        let target_position = axis
            .children
            .iter()
            .position(|child| matches!(child, PaneGroup::Pane(id) if *id == target));
        if let Some(position) = target_position {
            axis.children.remove(position);
            axis.ratios.remove(position);
            let total: f32 = axis.ratios.iter().sum();
            if total > 0. {
                for ratio in &mut axis.ratios {
                    *ratio /= total;
                }
            }
            return true;
        }
        axis.children
            .iter_mut()
            .any(|child| Self::remove_pane_from_node(child, target))
    }

    fn collapse_single_child_axes(node: &mut PaneGroup) {
        if let PaneGroup::Axis(axis) = node {
            for child in &mut axis.children {
                Self::collapse_single_child_axes(child);
            }
            if axis.children.len() == 1 {
                *node = axis.children.remove(0);
            }
        }
    }

    /// Move the divider after child `divider` of axis `axis_id` so the
    /// boundary sits at `fraction` of the axis extent (0..1). Clamped so no
    /// pane drops below [`MIN_PANE_RATIO`]. Returns true when ratios changed.
    pub fn resize(&mut self, axis_id: usize, divider: usize, fraction: f32) -> bool {
        if !fraction.is_finite() {
            return false;
        }
        Self::resize_in_node(&mut self.layout, axis_id, divider, fraction)
    }

    fn resize_in_node(
        node: &mut PaneGroup,
        axis_id: usize,
        divider: usize,
        fraction: f32,
    ) -> bool {
        let PaneGroup::Axis(axis) = node else {
            return false;
        };
        if axis.id == axis_id {
            if divider + 1 >= axis.ratios.len() {
                return false;
            }
            let before: f32 = axis.ratios[..divider].iter().sum();
            let pair = axis.ratios[divider] + axis.ratios[divider + 1];
            let margin = MIN_PANE_RATIO.min(pair / 2.);
            let boundary = fraction.clamp(before + margin, before + pair - margin);
            let changed = (boundary - before - axis.ratios[divider]).abs() > f32::EPSILON;
            axis.ratios[divider] = boundary - before;
            axis.ratios[divider + 1] = pair - axis.ratios[divider];
            return changed;
        }
        axis.children
            .iter_mut()
            .any(|child| Self::resize_in_node(child, axis_id, divider, fraction))
    }

    /// Close every tab in every pane. The pane layout itself is kept: tabs are
    /// per-changeset, the split arrangement is not.
    pub fn clear(&mut self) {
        for (_, pane) in &mut self.panes {
            pane.tabs.clear();
            pane.active = None;
            pane.preview = None;
        }
    }
}
```

Then add a temporary compat block so app.rs/tab_bar.rs compile until Task 4 (delete there):

```rust
/// Active-pane delegates kept only until every call site is pane-scoped
/// (removed by the pane-grid task in the same slice).
impl Workspace {
    pub fn tabs_compat(&self) -> &[Box<dyn WorkspaceItem>] {
        self.tabs(self.active_pane)
    }
}
```

**Note:** rather than rename call sites twice, prefer the cheaper route: in this step ALSO mechanically update the existing call sites in `src/workspace/tab_bar.rs`, `src/app.rs`, and `tests/smoke.rs` to pass a pane argument — `workspace.tabs(pane)` etc. — where `let pane = app.workspace.active_pane();`. In `tab_bar.rs` thread a `pane: PaneId` parameter through `render_tab_bar` and have `render_changeset_screen` pass `self.workspace.active_pane()` for now (Task 4 makes it per-pane). Skip the `tabs_compat` shim entirely if you do this — it exists only as a fallback if the mechanical update turns out too entangled.

- [ ] **Step 3: Update the existing unit tests and add the new ones**

The existing `mod tests` calls go pane-scoped. Add a helper and rewrite mechanically:

```rust
    fn pane(ws: &Workspace) -> PaneId {
        ws.active_pane()
    }

    fn paths(workspace: &Workspace) -> Vec<&str> {
        workspace
            .tabs(workspace.active_pane())
            .iter()
            .map(|tab| tab.path())
            .collect()
    }
```

(e.g. `ws.activate_tab(1)` → `ws.activate_tab(pane(&ws), 1)`, `ws.is_preview(0)` → `ws.is_preview(pane(&ws), 0)`, `ws.active_index()` → `ws.active_index(pane(&ws))`, `ws.active_item()` → `ws.active_item(pane(&ws))`.)

Append the new transition tests inside `mod tests`:

```rust
    fn layout_ratios(ws: &Workspace) -> Vec<f32> {
        match ws.layout() {
            PaneGroup::Axis(axis) => axis.ratios.clone(),
            PaneGroup::Pane(_) => panic!("expected an axis root"),
        }
    }

    #[test]
    fn new_workspace_is_a_single_active_pane() {
        let ws = Workspace::new();
        assert_eq!(ws.pane_ids(), [0]);
        assert_eq!(ws.active_pane(), 0);
        assert_eq!(ws.layout(), &PaneGroup::Pane(0));
    }

    #[test]
    fn split_right_adds_an_empty_active_pane_after_the_source() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        let new_pane = ws.split(0, SplitDirection::Right).expect("split");
        assert_eq!(ws.pane_ids(), [0, new_pane]);
        assert_eq!(ws.active_pane(), new_pane);
        assert!(ws.tabs(new_pane).is_empty());
        assert_eq!(paths_in(&ws, 0), ["a.rs"], "source pane keeps its tabs");
        assert_eq!(layout_ratios(&ws), [0.5, 0.5]);
        match ws.layout() {
            PaneGroup::Axis(axis) => assert_eq!(axis.axis, SplitAxis::Horizontal),
            PaneGroup::Pane(_) => panic!("expected an axis root"),
        }
    }

    #[test]
    fn split_left_and_up_insert_before_the_source() {
        let mut ws = Workspace::new();
        let left = ws.split(0, SplitDirection::Left).expect("split left");
        assert_eq!(ws.pane_ids(), [left, 0]);

        let mut ws = Workspace::new();
        let up = ws.split(0, SplitDirection::Up).expect("split up");
        assert_eq!(ws.pane_ids(), [up, 0]);
        match ws.layout() {
            PaneGroup::Axis(axis) => assert_eq!(axis.axis, SplitAxis::Vertical),
            PaneGroup::Pane(_) => panic!("expected an axis root"),
        }
    }

    #[test]
    fn same_axis_split_inserts_a_sibling_instead_of_nesting() {
        let mut ws = Workspace::new();
        let second = ws.split(0, SplitDirection::Right).expect("first split");
        let third = ws.split(0, SplitDirection::Right).expect("second split");
        assert_eq!(ws.pane_ids(), [0, third, second]);
        let ratios = layout_ratios(&ws);
        assert_eq!(ratios.len(), 3);
        assert!((ratios[0] - 0.25).abs() < 1e-6);
        assert!((ratios[1] - 0.25).abs() < 1e-6);
        assert!((ratios[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn cross_axis_split_nests_an_axis_node() {
        let mut ws = Workspace::new();
        let right = ws.split(0, SplitDirection::Right).expect("split right");
        let below = ws.split(right, SplitDirection::Down).expect("split down");
        assert_eq!(ws.pane_ids(), [0, right, below]);
        let PaneGroup::Axis(root) = ws.layout() else {
            panic!("expected an axis root");
        };
        assert_eq!(root.children.len(), 2);
        let PaneGroup::Axis(nested) = &root.children[1] else {
            panic!("expected a nested axis");
        };
        assert_eq!(nested.axis, SplitAxis::Vertical);
        assert_eq!(nested.children, vec![PaneGroup::Pane(right), PaneGroup::Pane(below)]);
    }

    #[test]
    fn split_of_unknown_pane_is_refused() {
        let mut ws = Workspace::new();
        assert_eq!(ws.split(99, SplitDirection::Right), None);
        assert_eq!(ws.pane_ids(), [0]);
    }

    #[test]
    fn tree_opens_go_to_the_active_pane() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        let new_pane = ws.split(0, SplitDirection::Right).expect("split");
        ws.open_preview(item("b.rs"));
        assert_eq!(paths_in(&ws, 0), ["a.rs"]);
        assert_eq!(paths_in(&ws, new_pane), ["b.rs"]);
        ws.activate_pane(0);
        ws.open_preview(item("c.rs"));
        assert_eq!(paths_in(&ws, 0), ["c.rs"], "preview replaced in pane 0");
    }

    #[test]
    fn a_file_may_be_open_in_two_panes_but_once_per_pane() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        let new_pane = ws.split(0, SplitDirection::Right).expect("split");
        ws.open_pinned(item("a.rs"));
        assert_eq!(paths_in(&ws, 0), ["a.rs"]);
        assert_eq!(paths_in(&ws, new_pane), ["a.rs"]);
        ws.open_pinned(item("a.rs"));
        assert_eq!(paths_in(&ws, new_pane), ["a.rs"], "no duplicate within a pane");
    }

    #[test]
    fn close_pane_collapses_the_axis_and_returns_space() {
        let mut ws = Workspace::new();
        let right = ws.split(0, SplitDirection::Right).expect("split");
        assert!(ws.close_pane(right));
        assert_eq!(ws.layout(), &PaneGroup::Pane(0));
        assert_eq!(ws.active_pane(), 0);
    }

    #[test]
    fn close_pane_renormalizes_sibling_ratios() {
        let mut ws = Workspace::new();
        let second = ws.split(0, SplitDirection::Right).expect("first split");
        let _third = ws.split(0, SplitDirection::Right).expect("second split");
        // ratios are [0.25, 0.25, 0.5]; closing the 0.5 pane renormalizes.
        assert!(ws.close_pane(second));
        let ratios = layout_ratios(&ws);
        assert_eq!(ratios.len(), 2);
        assert!((ratios.iter().sum::<f32>() - 1.).abs() < 1e-6);
        assert!((ratios[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn close_pane_refuses_the_last_pane_and_unknown_ids() {
        let mut ws = Workspace::new();
        assert!(!ws.close_pane(0));
        assert!(!ws.close_pane(42));
        assert_eq!(ws.pane_ids(), [0]);
    }

    #[test]
    fn closing_the_active_pane_activates_the_pane_taking_its_place() {
        let mut ws = Workspace::new();
        let right = ws.split(0, SplitDirection::Right).expect("split");
        assert_eq!(ws.active_pane(), right);
        assert!(ws.close_pane(right));
        assert_eq!(ws.active_pane(), 0);
    }

    #[test]
    fn closing_a_nested_pane_collapses_only_its_axis() {
        let mut ws = Workspace::new();
        let right = ws.split(0, SplitDirection::Right).expect("split right");
        let below = ws.split(right, SplitDirection::Down).expect("split down");
        assert!(ws.close_pane(right));
        assert_eq!(ws.pane_ids(), [0, below]);
        let PaneGroup::Axis(root) = ws.layout() else {
            panic!("expected an axis root");
        };
        assert_eq!(root.children[1], PaneGroup::Pane(below), "nested axis collapsed");
    }

    #[test]
    fn resize_moves_the_divider_and_clamps_to_the_minimum_ratio() {
        let mut ws = Workspace::new();
        ws.split(0, SplitDirection::Right);
        let PaneGroup::Axis(axis) = ws.layout() else {
            panic!("expected an axis root");
        };
        let axis_id = axis.id;
        assert!(ws.resize(axis_id, 0, 0.7));
        assert!((layout_ratios(&ws)[0] - 0.7).abs() < 1e-6);
        assert!(ws.resize(axis_id, 0, 0.01), "clamps instead of refusing");
        assert!((layout_ratios(&ws)[0] - MIN_PANE_RATIO).abs() < 1e-6);
        assert!(ws.resize(axis_id, 0, 1.5));
        assert!((layout_ratios(&ws)[1] - MIN_PANE_RATIO).abs() < 1e-6);
    }

    #[test]
    fn resize_refuses_bad_input() {
        let mut ws = Workspace::new();
        ws.split(0, SplitDirection::Right);
        let PaneGroup::Axis(axis) = ws.layout() else {
            panic!("expected an axis root");
        };
        let axis_id = axis.id;
        assert!(!ws.resize(999, 0, 0.5), "unknown axis");
        assert!(!ws.resize(axis_id, 5, 0.5), "divider out of range");
        assert!(!ws.resize(axis_id, 0, f32::NAN), "non-finite fraction");
    }

    #[test]
    fn cycle_wraps_within_the_active_pane() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        assert!(ws.activate_next_tab());
        assert_eq!(ws.active_index(pane(&ws)), Some(0), "wraps past the end");
        assert!(ws.activate_previous_tab());
        assert_eq!(ws.active_index(pane(&ws)), Some(2), "wraps back");
    }

    #[test]
    fn cycle_is_a_no_op_with_fewer_than_two_tabs() {
        let mut ws = Workspace::new();
        assert!(!ws.activate_next_tab());
        ws.open_preview(item("a.rs"));
        assert!(!ws.activate_next_tab());
        assert!(!ws.activate_previous_tab());
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
    }

    #[test]
    fn clear_empties_every_pane_but_keeps_the_layout() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        let right = ws.split(0, SplitDirection::Right).expect("split");
        ws.open_pinned(item("b.rs"));
        ws.clear();
        assert!(ws.tabs(0).is_empty());
        assert!(ws.tabs(right).is_empty());
        assert_eq!(ws.pane_ids(), [0, right], "layout survives clear");
        assert_eq!(ws.active_pane(), right);
    }
```

Add the helper used above:

```rust
    fn paths_in(workspace: &Workspace, pane: PaneId) -> Vec<&str> {
        workspace.tabs(pane).iter().map(|tab| tab.path()).collect()
    }
```

- [ ] **Step 4: Mechanically update call sites in `app.rs`, `tab_bar.rs`, `tests/smoke.rs`**

Pass the active pane everywhere a no-pane method was called (`render_tab_bar` gains a `pane: PaneId` parameter, `render_changeset_screen` passes `self.workspace.active_pane()` and renders the active pane's tab bar only — full multi-pane rendering is Task 4). App tab methods (`activate_workspace_tab` etc.) gain a `pane: PaneId` first parameter; `tab_bar.rs` listeners capture and pass it. Test selectors stay unchanged in this task.

Create `src/workspace/pane_grid.rs` as a placeholder so the module declaration compiles:

```rust
//! Recursive renderer for the workspace pane tree. Implemented with the
//! pane-grid task of slice 2; state lives in the parent module.
```

- [ ] **Step 5: Run the tests**

Run: `cargo test`
Expected: PASS (all existing + ~18 new unit tests).

- [ ] **Step 6: Commit**

```bash
git add src/workspace/mod.rs src/workspace/pane_grid.rs src/workspace/tab_bar.rs src/app.rs tests/smoke.rs
git commit -m "feat(workspace): ratio-weighted pane tree with split, collapse, and resize"
```

---

### Task 3: Per-pane scroll state in `App`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Replace the two scroll fields with a per-pane map**

Remove fields `tab_bar_scroll: ScrollHandle` and `file_diff_scroll: FileDiffScroll`; add:

```rust
    /// Scroll handles per pane (tab strip + diff sides), created on demand.
    /// RefCell because render paths take `&self`; ScrollHandle clones share
    /// their underlying state, so handing out clones is safe.
    pane_scrolls: RefCell<HashMap<crate::workspace::PaneId, PaneScrollState>>,
```

with (next to `FileDiffScroll`):

```rust
#[derive(Clone)]
pub(crate) struct PaneScrollState {
    pub(crate) tab_bar: ScrollHandle,
    pub(crate) diff: FileDiffScroll,
}

impl PaneScrollState {
    fn new() -> Self {
        Self {
            tab_bar: ScrollHandle::new(),
            diff: FileDiffScroll::new(),
        }
    }
}
```

Make `FileDiffScroll` `#[derive(Clone)]` and `pub(crate)` (struct and the `handle_for`/`reset` methods stay as-is; `pane_grid.rs` will pass it around). Add `use std::cell::RefCell;` and `use std::collections::HashMap;` (check existing imports first — `BTreeMap`/`BTreeSet` exist; `HashMap` may not).

Constructor literal: replace the two old field initializers with `pane_scrolls: RefCell::new(HashMap::new()),`.

- [ ] **Step 2: Add accessors and reset helper**

```rust
    pub(crate) fn pane_scroll(&self, pane: crate::workspace::PaneId) -> PaneScrollState {
        self.pane_scrolls
            .borrow_mut()
            .entry(pane)
            .or_insert_with(PaneScrollState::new)
            .clone()
    }

    fn reset_pane_scrolls(&self) {
        for state in self.pane_scrolls.borrow().values() {
            state.diff.reset();
            state.tab_bar.set_offset(point(px(0.), px(0.)));
        }
    }
```

- [ ] **Step 3: Rewrite the call sites**

- `open_file_preview` / `open_file_pinned`:

```rust
    fn open_file_preview(&mut self, path: String, cx: &mut Context<Self>) {
        self.file_tree_highlight_path = Some(path.clone());
        let pane = self.workspace.active_pane();
        if self
            .workspace
            .open_preview(Box::new(FileDiffItem::new(path)))
        {
            self.pane_scroll(pane).diff.reset();
        }
        if let Some(index) = self.workspace.active_index(pane) {
            self.pane_scroll(pane).tab_bar.scroll_to_item(index);
        }
        cx.notify();
    }
```

(`open_file_pinned` identically with `open_pinned`.)

- `activate_workspace_tab` / `promote_workspace_tab` / `close_workspace_tab` (already pane-scoped from Task 2): replace `self.file_diff_scroll.reset()` with `self.pane_scroll(pane).diff.reset()` and `self.tab_bar_scroll.scroll_to_item(index)` with `self.pane_scroll(pane).tab_bar.scroll_to_item(index)`.
- `apply_open_repository`, `open_changeset`, `close_changeset`: replace the `self.file_diff_scroll.reset();` + `self.tab_bar_scroll.set_offset(point(px(0.), px(0.)));` pairs with `self.reset_pane_scrolls();`.
- `render_changeset_screen`: pass `&self.pane_scroll(active_pane).tab_bar` to `render_tab_bar` (bind the `PaneScrollState` to a local first — it is an owned clone).
- `render_file_detail`, `render_changed_file_detail`, `render_read_only_file_detail`: add a `scroll: &FileDiffScroll` parameter and use it instead of `&self.file_diff_scroll`; `render_changeset_screen` passes `&self.pane_scroll(active_pane).diff`.
- Any other `self.file_diff_scroll` / `self.tab_bar_scroll` reads (search both names; e.g. the diff scroll test helpers) become `self.pane_scroll(pane)` with the relevant pane (in tests: `app.workspace.active_pane()`). `pane_scroll` is `pub(crate)`, so in-crate tests can call it.

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "refactor(app): per-pane scroll state behind a RefCell map"
```

---

### Task 4: Pane grid renderer, pane-scoped tab bar, split controls

**Files:**
- Modify: `src/workspace/pane_grid.rs` (full implementation + view tests)
- Modify: `src/workspace/tab_bar.rs` (pane-scoped selectors, dimming, corner controls)
- Modify: `src/app.rs` (pane methods; mount the grid)
- Modify: `tests/smoke.rs` (selector updates + split coverage)

- [ ] **Step 1: Add the pane-level App methods**

In `src/app.rs` (near the tab methods):

```rust
    pub(crate) fn activate_workspace_pane(
        &mut self,
        pane: crate::workspace::PaneId,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.activate_pane(pane) {
            cx.notify();
        }
    }

    pub(crate) fn split_workspace_pane(
        &mut self,
        pane: crate::workspace::PaneId,
        direction: crate::workspace::SplitDirection,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.split(pane, direction).is_some() {
            cx.notify();
        }
    }

    pub(crate) fn close_workspace_pane(
        &mut self,
        pane: crate::workspace::PaneId,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.close_pane(pane) {
            self.pane_scrolls.borrow_mut().remove(&pane);
            cx.notify();
        }
    }

    pub(crate) fn resize_workspace_divider(
        &mut self,
        axis_id: usize,
        divider: usize,
        fraction: f32,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.resize(axis_id, divider, fraction) {
            cx.notify();
        }
    }
```

- [ ] **Step 2: Implement `pane_grid.rs`**

```rust
//! Recursive renderer for the workspace pane tree.
//!
//! Axis nodes render as ratio-weighted flex rows/columns with draggable
//! dividers; pane leaves render a tab bar above the diff content. Exactly one
//! pane is active; the others render dimmed tab bars. Behavior contract lives
//! in `docs/specs/review/workflow.md`.

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, relative, rgb, AnyElement, Context, DragMoveEvent, Empty, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window,
};

use super::{AxisNode, PaneGroup, PaneId, SplitAxis};
use crate::app::App;
use crate::repo;

const DIVIDER_THICKNESS: f32 = 4.;
const DIVIDER_COLOR: u32 = 0x2a2a2a;
const DIVIDER_HOVER_COLOR: u32 = 0x7da4ff;

/// Dragging a divider between two children of an axis node.
#[derive(Clone)]
pub(crate) struct DraggedDivider {
    pub axis_id: usize,
    pub divider: usize,
}

/// Invisible drag preview: divider drags give feedback by live-resizing.
pub(crate) struct EmptyDragPreview;

impl Render for EmptyDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub fn render_pane_group(
    app: &App,
    node: &PaneGroup,
    repo: &repo::OpenRepository,
    changeset: &repo::ChangeSet,
    cx: &mut Context<App>,
) -> AnyElement {
    match node {
        PaneGroup::Pane(pane) => render_pane(app, *pane, repo, changeset, cx),
        PaneGroup::Axis(axis) => render_axis(app, axis, repo, changeset, cx),
    }
}

fn render_pane(
    app: &App,
    pane: PaneId,
    repo: &repo::OpenRepository,
    changeset: &repo::ChangeSet,
    cx: &mut Context<App>,
) -> AnyElement {
    let scrolls = app.pane_scroll(pane);
    let active_path = app
        .workspace
        .active_item(pane)
        .map(|item| item.path().to_string());

    div()
        .id(("workspace-pane", pane))
        .debug_selector(move || format!("workspace-pane-{pane}"))
        .flex()
        .flex_col()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, _event: &MouseDownEvent, _window, cx| {
                app.activate_workspace_pane(pane, cx);
            }),
        )
        .child(super::tab_bar::render_tab_bar(
            &app.workspace,
            pane,
            changeset,
            &scrolls.tab_bar,
            cx,
        ))
        .child(app.render_file_detail(repo, changeset, active_path.as_deref(), &scrolls.diff))
        .into_any_element()
}

fn render_axis(
    app: &App,
    axis: &AxisNode,
    repo: &repo::OpenRepository,
    changeset: &repo::ChangeSet,
    cx: &mut Context<App>,
) -> AnyElement {
    let axis_id = axis.id;
    let direction = axis.axis;

    let mut container = div()
        .id(SharedString::from(format!("workspace-axis-{axis_id}")))
        .debug_selector(move || format!("workspace-axis-{axis_id}"))
        .flex()
        .size_full()
        .min_w_0()
        .min_h_0()
        .map(|container| match direction {
            SplitAxis::Horizontal => container.flex_row(),
            SplitAxis::Vertical => container.flex_col(),
        })
        .on_drag_move(cx.listener(
            move |app, event: &DragMoveEvent<DraggedDivider>, _window, cx| {
                let (event_axis_id, divider) = {
                    let drag = event.drag(cx);
                    (drag.axis_id, drag.divider)
                };
                if event_axis_id != axis_id {
                    return;
                }
                let bounds = event.bounds;
                let position = event.event.position;
                let fraction = match direction {
                    SplitAxis::Horizontal => {
                        (position.x - bounds.left()) / bounds.size.width
                    }
                    SplitAxis::Vertical => (position.y - bounds.top()) / bounds.size.height,
                };
                app.resize_workspace_divider(axis_id, divider, fraction, cx);
            },
        ));

    for (index, child) in axis.children.iter().enumerate() {
        if index > 0 {
            container = container.child(render_divider(axis_id, direction, index - 1, cx));
        }
        container = container.child(
            div()
                .flex()
                .min_w_0()
                .min_h_0()
                .flex_basis(relative(axis.ratios[index]))
                .child(render_pane_group(app, child, repo, changeset, cx)),
        );
    }

    container.into_any_element()
}

fn render_divider(
    axis_id: usize,
    direction: SplitAxis,
    divider: usize,
    cx: &mut Context<App>,
) -> AnyElement {
    let selector = format!("workspace-divider-{axis_id}-{divider}");
    let _ = cx;
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .flex_none()
        .map(|handle| match direction {
            SplitAxis::Horizontal => handle.w(px(DIVIDER_THICKNESS)).h_full().cursor_col_resize(),
            SplitAxis::Vertical => handle.h(px(DIVIDER_THICKNESS)).w_full().cursor_row_resize(),
        })
        .bg(rgb(DIVIDER_COLOR))
        .hover(|handle| handle.bg(rgb(DIVIDER_HOVER_COLOR)))
        .on_drag(
            DraggedDivider { axis_id, divider },
            |_drag, _offset, _window, cx| cx.new(|_| EmptyDragPreview),
        )
        .into_any_element()
}
```

(If `cx` ends up unused in `render_divider`, drop the parameter instead of keeping `let _ = cx;`.)

- [ ] **Step 3: Rework `tab_bar.rs`**

Changes to `render_tab_bar(workspace, pane, changeset, scroll, cx)`:

1. **Always render the 32px bar** inside a changeset (so empty panes still offer split controls). When the pane has no tabs, the strip holds the zero-size marker `workspace-tab-bar-empty-{pane}` instead of tabs.
2. **Pane-scoped selectors:** `workspace-tab-{pane}-{index}`, `workspace-tab-close-{pane}-{index}`, group name `workspace-tab-{pane}-{index}`, bar `workspace-tab-bar-{pane}`.
3. **Dimming:** `let pane_active = workspace.active_pane() == pane;` — when inactive, the bar gets `.opacity(0.6)` (`TAB_BAR_INACTIVE_OPACITY` const).
4. **Layout:** the bar becomes a row of `[scrollable strip (flex_1, min_w_0, overflow_x_scroll, track_scroll), corner controls (flex_none)]`; tab children move into the strip.
5. **Corner split controls** (right corner, after the strip):

```rust
fn corner_control(
    pane: PaneId,
    icon: LucideIcon,
    selector: String,
    direction: SplitDirection,
    cx: &mut Context<App>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .flex()
        .items_center()
        .justify_center()
        .w(px(24.))
        .h(px(24.))
        .rounded(px(2.))
        .cursor_pointer()
        .hover(|button| button.bg(rgb(TAB_BORDER)))
        .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
            app.split_workspace_pane(pane, direction, cx);
        }))
        .child(
            Icon::new(icon)
                .size(px(14.))
                .text_color(rgb(TAB_MUTED_TEXT)),
        )
}
```

mounted as:

```rust
        .child(
            div()
                .flex()
                .items_center()
                .flex_none()
                .gap_1()
                .px_2()
                .h_full()
                .child(corner_control(
                    pane,
                    LucideIcon::Columns2,
                    format!("workspace-split-right-{pane}"),
                    SplitDirection::Right,
                    cx,
                ))
                .child(corner_control(
                    pane,
                    LucideIcon::Rows2,
                    format!("workspace-split-down-{pane}"),
                    SplitDirection::Down,
                    cx,
                )),
        )
```

6. Tab listeners pass `pane` to `activate_workspace_tab(pane, index, cx)` / `promote_workspace_tab` / `close_workspace_tab`.

Imports to add: `SharedString`, `SplitDirection`, `PaneId` from `super`.

- [ ] **Step 4: Mount the grid in `render_changeset_screen`**

Replace the right `resizable_panel()` child (the tab bar + detail column) with:

```rust
                            .child(resizable_panel().child(
                                crate::workspace::pane_grid::render_pane_group(
                                    self,
                                    self.workspace.layout(),
                                    repo,
                                    changeset,
                                    cx,
                                ),
                            )),
```

Delete the now-unused `active_path` binding in `render_changeset_screen` (pane content paths come from `render_pane`). `render_file_detail` must be `pub(crate)` for `pane_grid.rs`.

- [ ] **Step 5: Update existing tests for the pane-scoped selectors**

In `src/workspace/tab_bar.rs` tests and `tests/smoke.rs`: `workspace-tab-0` → `workspace-tab-0-0`, `workspace-tab-1` → `workspace-tab-0-1`, `workspace-tab-close-1` → `workspace-tab-close-0-1`, `workspace-tab-bar-empty` → `workspace-tab-bar-empty-0`. State assertions go through `let pane = app.workspace.active_pane();`.

- [ ] **Step 6: Write the pane-grid view tests**

Append to `src/workspace/pane_grid.rs` (fixtures mirror `tab_bar.rs`; extract shared helpers into `src/workspace/test_util.rs`-style reuse only if trivial — otherwise duplicate the small fixture, matching the existing test convention):

```rust
#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::workspace::{PaneGroup, SplitDirection};
    use gpui::{Modifiers, MouseButton, Point, TestAppContext, VisualTestContext, WindowHandle};

    // ... copy of add_app_window / commit_all / init_repo_with_two_changed_files /
    // open_changeset helpers from tab_bar.rs tests ...

    #[gpui::test]
    async fn split_right_control_adds_a_pane_to_the_right(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split-right control renders");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        let pane0 = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        let pane1 = visual.debug_bounds("workspace-pane-1").expect("pane 1");
        assert!(
            pane1.left() >= pane0.right(),
            "new pane sits to the right of the source"
        );
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids(), [0, 1]);
                assert_eq!(app.workspace.active_pane(), 1, "new pane is active");
                assert!(app.workspace.tabs(1).is_empty(), "new pane starts empty");
            })
            .expect("read workspace state");
        visual
            .debug_bounds("file-detail-empty")
            .expect("empty pane shows the placeholder");
    }

    #[gpui::test]
    async fn split_down_control_adds_a_pane_below(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let split = visual
            .debug_bounds("workspace-split-down-0")
            .expect("split-down control renders");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        let pane0 = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        let pane1 = visual.debug_bounds("workspace-pane-1").expect("pane 1");
        assert!(pane1.top() >= pane0.bottom(), "new pane sits below the source");
        window
            .read_with(cx, |app, _cx| assert_eq!(app.workspace.active_pane(), 1))
            .expect("read active pane");
    }

    #[gpui::test]
    async fn tree_clicks_open_in_the_active_pane_and_clicks_activate_panes(
        cx: &mut TestAppContext,
    ) {
        let (_dir, window, mut visual) = open_changeset(cx);

        // Split: pane 1 becomes active; the tree click lands there.
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();
        click_file_row(&mut visual, 0);
        cx.run_until_parked();

        visual
            .debug_bounds("workspace-tab-1-0")
            .expect("tab opened in pane 1");
        window
            .read_with(cx, |app, _cx| {
                assert!(app.workspace.tabs(0).is_empty());
                assert_eq!(app.workspace.tabs(1).len(), 1);
            })
            .expect("read tab placement");

        // Clicking inside pane 0 activates it; the next open lands there.
        let pane0 = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        visual.simulate_click(pane0.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| assert_eq!(app.workspace.active_pane(), 0))
            .expect("read activation");
        click_file_row(&mut visual, 1);
        cx.run_until_parked();
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1, "open routed to pane 0");
            })
            .expect("read routed open");
    }

    #[gpui::test]
    async fn dragging_the_divider_resizes_the_panes(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        let divider = visual
            .debug_bounds("workspace-divider-2-0")
            .expect("divider renders (axis id 2)");
        let start = divider.center();
        let end = Point {
            x: start.x + gpui::px(80.),
            y: start.y,
        };
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_move(start + gpui::point(gpui::px(4.), gpui::px(0.)), MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let PaneGroup::Axis(axis) = app.workspace.layout() else {
                    panic!("expected an axis root");
                };
                assert!(
                    axis.ratios[0] > 0.5,
                    "left pane grew: ratios {:?}",
                    axis.ratios
                );
            })
            .expect("read resized ratios");
    }

    #[gpui::test]
    async fn inactive_pane_tab_bar_is_dimmed(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        click_file_row(&mut visual, 0);
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();
        // Pane 1 is active; pane 0's bar renders (dimmed) and pane 0's tab is
        // still clickable, which re-activates pane 0.
        let tab = visual
            .debug_bounds("workspace-tab-0-0")
            .expect("pane 0 tab still renders");
        visual.simulate_click(tab.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.active_pane(), 0, "clicking a tab activates its pane");
            })
            .expect("read activation");
    }
}
```

Notes:
- Dimming itself is a style (opacity), which `debug_bounds` cannot read; the test asserts the behavioral half (bar renders, clicks activate). Verify the opacity visually via the constant + code review.
- Axis id 2 in the divider test: ids 0 (first pane), 1 (new pane), 2 (axis) — the shared counter. If this proves brittle, read the id from `app.workspace.layout()` first.
- `gpui::point(...)` constructs a `Point`; `+` on points is available.

- [ ] **Step 7: Extend the smoke test**

In `tests/smoke.rs`, after the pin assertions: click `workspace-split-right-0`, click a second file row, assert `workspace-tab-1-0` exists and `app.workspace.pane_ids() == [0, 1]`.

- [ ] **Step 8: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/workspace src/app.rs tests/smoke.rs
git commit -m "feat(workspace): pane grid with splits, dividers, and active-pane routing"
```

---

### Task 5: Keyboard bindings

**Files:**
- Modify: `src/app.rs` (actions + handlers)
- Modify: `src/app/menu.rs` (bindings)
- Test: `src/workspace/pane_grid.rs` (keyboard view tests)

- [ ] **Step 1: Define the actions**

Extend the `actions!` list in `src/app.rs`:

```rust
actions!(
    app,
    [
        OpenRepository,
        OpenChangeset,
        CloseChangeset,
        QuitApplication,
        CloseActiveTab,
        ActivateNextTab,
        ActivatePreviousTab,
        SplitPaneLeft,
        SplitPaneRight,
        SplitPaneUp,
        SplitPaneDown,
        CloseActivePane
    ]
);
```

- [ ] **Step 2: Add the bindings in `menu.rs`**

```rust
pub const CLOSE_ACTIVE_TAB_KEYSTROKE: &str = "cmd-w";
pub const ACTIVATE_NEXT_TAB_KEYSTROKE: &str = "ctrl-tab";
pub const ACTIVATE_PREVIOUS_TAB_KEYSTROKE: &str = "ctrl-shift-tab";
pub const SPLIT_PANE_LEFT_KEYSTROKE: &str = "cmd-k left";
pub const SPLIT_PANE_RIGHT_KEYSTROKE: &str = "cmd-k right";
pub const SPLIT_PANE_UP_KEYSTROKE: &str = "cmd-k up";
pub const SPLIT_PANE_DOWN_KEYSTROKE: &str = "cmd-k down";
pub const CLOSE_ACTIVE_PANE_KEYSTROKE: &str = "cmd-k w";
```

Extend `bind_app_keys`:

```rust
pub fn bind_app_keys(cx: &mut GpuiApp) {
    cx.bind_keys([
        open_repository_key_binding(),
        quit_application_key_binding(),
        KeyBinding::new(CLOSE_ACTIVE_TAB_KEYSTROKE, CloseActiveTab, None),
        KeyBinding::new(ACTIVATE_NEXT_TAB_KEYSTROKE, ActivateNextTab, None),
        KeyBinding::new(ACTIVATE_PREVIOUS_TAB_KEYSTROKE, ActivatePreviousTab, None),
        KeyBinding::new(SPLIT_PANE_LEFT_KEYSTROKE, SplitPaneLeft, None),
        KeyBinding::new(SPLIT_PANE_RIGHT_KEYSTROKE, SplitPaneRight, None),
        KeyBinding::new(SPLIT_PANE_UP_KEYSTROKE, SplitPaneUp, None),
        KeyBinding::new(SPLIT_PANE_DOWN_KEYSTROKE, SplitPaneDown, None),
        KeyBinding::new(CLOSE_ACTIVE_PANE_KEYSTROKE, CloseActivePane, None),
    ]);
}
```

(Import the new actions in `menu.rs`'s `use super::{...}` list.)

- [ ] **Step 3: Add the handlers**

App methods (all no-ops outside an open changeset):

```rust
    fn changeset_open(&self) -> bool {
        matches!(self.mode, Mode::RepoOpen { .. })
            && matches!(self.review_screen, ReviewScreen::Changeset { .. })
    }

    fn close_active_workspace_tab(&mut self, cx: &mut Context<Self>) {
        if !self.changeset_open() {
            return;
        }
        let pane = self.workspace.active_pane();
        if let Some(index) = self.workspace.active_index(pane) {
            self.close_workspace_tab(pane, index, cx);
        }
    }

    fn cycle_workspace_tab(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.changeset_open() {
            return;
        }
        let changed = if forward {
            self.workspace.activate_next_tab()
        } else {
            self.workspace.activate_previous_tab()
        };
        if changed {
            let pane = self.workspace.active_pane();
            self.pane_scroll(pane).diff.reset();
            if let Some(index) = self.workspace.active_index(pane) {
                self.pane_scroll(pane).tab_bar.scroll_to_item(index);
            }
            cx.notify();
        }
    }

    fn split_active_workspace_pane(
        &mut self,
        direction: crate::workspace::SplitDirection,
        cx: &mut Context<Self>,
    ) {
        if !self.changeset_open() {
            return;
        }
        let pane = self.workspace.active_pane();
        self.split_workspace_pane(pane, direction, cx);
    }

    fn close_active_workspace_pane(&mut self, cx: &mut Context<Self>) {
        if !self.changeset_open() {
            return;
        }
        let pane = self.workspace.active_pane();
        self.close_workspace_pane(pane, cx);
    }
```

`impl Render` on_action chain additions:

```rust
            .on_action(cx.listener(|app, _: &CloseActiveTab, _window, cx| {
                app.close_active_workspace_tab(cx);
            }))
            .on_action(cx.listener(|app, _: &ActivateNextTab, _window, cx| {
                app.cycle_workspace_tab(true, cx);
            }))
            .on_action(cx.listener(|app, _: &ActivatePreviousTab, _window, cx| {
                app.cycle_workspace_tab(false, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneLeft, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Left, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneRight, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Right, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneUp, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Up, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneDown, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Down, cx);
            }))
            .on_action(cx.listener(|app, _: &CloseActivePane, _window, cx| {
                app.close_active_workspace_pane(cx);
            }))
```

- [ ] **Step 4: Keyboard view tests**

Find how the existing quit-keystroke test registers bindings (search `bind_app_keys` / `QuitRequested` in `src/app.rs` tests) and replicate. Append to `pane_grid.rs` tests:

```rust
    #[gpui::test]
    async fn keyboard_drives_tabs_and_panes(cx: &mut TestAppContext) {
        cx.update(|cx| crate::app::menu::bind_app_keys(cx)); // match the existing pattern
        let (_dir, window, mut visual) = open_changeset(cx);

        // Two tabs in pane 0 (pin first, preview second).
        let row = alpha_row_bounds(&mut visual);
        crate::workspace::test_util::simulate_double_click(&mut visual, row.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Ctrl+Tab cycles (wraps from index 1 back to 0).
        visual.simulate_keystrokes("ctrl-tab");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.active_index(0), Some(0));
            })
            .expect("ctrl-tab cycles forward");
        visual.simulate_keystrokes("ctrl-shift-tab");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.active_index(0), Some(1));
            })
            .expect("ctrl-shift-tab cycles back");

        // Cmd+W closes the active tab.
        visual.simulate_keystrokes("cmd-w");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1);
            })
            .expect("cmd-w closes the active tab");

        // Cmd+K right splits; Cmd+K w closes the new pane again.
        visual.simulate_keystrokes("cmd-k right");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids().len(), 2);
                assert_eq!(app.workspace.active_pane(), 1);
            })
            .expect("cmd-k right splits");
        visual.simulate_keystrokes("cmd-k w");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids(), [0]);
                assert_eq!(app.workspace.active_pane(), 0);
            })
            .expect("cmd-k w closes the pane");
    }

    #[gpui::test]
    async fn cmd_k_down_splits_vertically(cx: &mut TestAppContext) {
        cx.update(|cx| crate::app::menu::bind_app_keys(cx));
        let (_dir, window, mut visual) = open_changeset(cx);
        visual.simulate_keystrokes("cmd-k down");
        window
            .read_with(cx, |app, _cx| {
                use crate::workspace::{PaneGroup, SplitAxis};
                let PaneGroup::Axis(axis) = app.workspace.layout() else {
                    panic!("expected an axis root");
                };
                assert_eq!(axis.axis, SplitAxis::Vertical);
            })
            .expect("cmd-k down splits vertically");
    }
```

If `menu` is not `pub` from `crate::app`, make `pub mod menu;` (check `src/app.rs` module declarations for how `menu` is declared; smoke tests may already import it).

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/app/menu.rs src/workspace/pane_grid.rs
git commit -m "feat(workspace): keyboard bindings for tabs and panes"
```

---

### Task 6: Update the review workflow spec

**Files:**
- Modify: `docs/specs/review/workflow.md`

- [ ] **Step 1: Re-read the voice rules** (`docs/specs/README.md`, `docs/guides/writing.md`).

- [ ] **Step 2: Adjust the tabs section** ("Holding files open in tabs"): the close-everything outcome now reads per pane, e.g. change `Closing every tab returns the diff area to the select-a-file placeholder.` to `Closing every tab in a pane returns that pane to the select-a-file placeholder; the tab row stays, holding the split controls.` and add `tabs live in the pane the user is working in` phrasing where the section presumes a single tab row.

- [ ] **Step 3: Add a new section** after "Holding files open in tabs" (before "Inspecting a file's diff"):

```markdown
## Splitting the diff area into panes

The diff area can hold several panes at once, arranged by vertical and horizontal splits, so two files — or two parts of one review — sit side by side. Each pane has its own tab row and its own diff. Exactly one pane is active at a time: it is where file-tree clicks open tabs, and its tab row renders at full strength while the other panes' rows are dimmed.

**Triggering conditions**

- The user clicks a split control in a pane's tab row, presses Cmd+K followed by an arrow key, clicks inside a pane, drags a divider, or closes a pane with Cmd+K W.

**Observable outcomes**

- Every pane's tab row carries split-right and split-down controls at its right edge, present even when the pane has no tabs.
- Splitting inserts a new empty pane next to the source pane — after it for right/down, before it for left/up — showing the select-a-file placeholder. The new pane becomes active and takes half the source pane's space; splitting along an existing row or column of panes adds a sibling rather than nesting.
- Clicking anywhere within a pane — its tab row or its content — makes it the active pane.
- Single- and double-clicks in the file tree open files in the active pane only. A file may be open in several panes at once, but never twice in the same pane.
- Cmd+W closes the active pane's active tab. Ctrl+Tab and Ctrl+Shift+Tab cycle through the active pane's tabs, wrapping at the ends. Cmd+K then an arrow key splits the active pane in that direction. Cmd+K W closes the active pane.
- Dividers between panes can be dragged to trade space between the neighbors they separate; a pane never shrinks below a tenth of its row or column.
- Closing a pane's last tab does not close the pane; it shows the placeholder. Closing a pane (Cmd+K W) removes it and returns its space to its siblings; the closed slot's neighbor becomes active.
- The last remaining pane cannot be closed.
- The split arrangement survives leaving and re-entering changesets within a session; the panes come back empty because tabs belong to a single changeset.

**Edge cases**

- Keyboard shortcuts do nothing while no changeset is open.
- Closing a pane that leaves a single child of a split collapses that level of the layout entirely.
```

- [ ] **Step 4: Commit**

```bash
git add docs/specs/review/workflow.md
git commit -m "docs(spec): pane splits in the review workflow"
```

---

### Task 7: Final verification

- [ ] **Step 1:** Run `bin/check`. Fix anything it flags (no `#[allow]` without user approval).
- [ ] **Step 2:** Re-read the design's "Panes and Splits" + "Visual Design" sections; confirm each slice-2 behavior has a landed test. Drag-and-drop of tabs and persistence must NOT appear.
- [ ] **Step 3:** Commit fixes if any: `git commit -m "fix(workspace): address bin/check findings"`.

---

## Self-Review

- **Spec coverage:** split controls ✓ (Task 4), one active pane + dimming + routing ✓ (Tasks 2/4), close-pane collapse ✓ (Task 2), keyboard ✓ (Task 5), draggable dividers ✓ (Tasks 2/4 — custom drag, deviation documented), spec doc ✓ (Task 6). Empty-pane placeholder ✓ (render_pane renders `file-detail-empty` when no active item).
- **Type consistency:** `split(PaneId, SplitDirection) -> Option<PaneId>`, `resize(usize, usize, f32) -> bool`, `pane_scroll(&self, PaneId) -> PaneScrollState` used consistently across tasks.
- **Known risk:** divider-drag in tests depends on gpui activating drags from simulated events (down + >2px move). If the drag doesn't activate, fall back to asserting via `app.resize_workspace_divider` invoked through a real `DragMoveEvent` path or test `Workspace::resize` coverage plus a divider-exists view assertion, and note the gap.
