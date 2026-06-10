# Diff Workspace Tabs — Slice 3 Implementation Plan (Drag and Drop)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tabs can be dragged: reorder within a pane (drop indicator at the insertion point), move between panes (merge on duplicate, moved preview becomes pinned), drag to a pane's edge zone to split with the dragged tab, and dragging a pane's last tab out collapses that pane.

**Architecture:** Two pure `Workspace` operations — `move_tab` and `split_with_tab` — carry all semantics, including last-tab-out pane collapse. Rendering adds a `DraggedTab` drag value: tabs become drag sources and drop targets, the tab strip accepts end-of-row drops, and each pane's content area tracks edge-zone hover in a new `App.tab_drop_zone` field (set from `on_drag_move` zone math, rendered as a half-pane highlight overlay, consumed by `on_drop`). Spec: `docs/superpowers/specs/2026-06-09-diff-workspace-tabs-design.md` (slice 3 only).

**Tech Stack:** Rust, gpui 0.2.2. Verification: `bin/check`.

---

## Verified API facts (carried from slice 2; do not re-derive)

- `on_drag(value, constructor)` needs `.id(...)` (StatefulInteractiveElement); constructor returns `Entity<impl Render>` used as the cursor-following preview, positioned at `mouse_position - cursor_offset`.
- `on_drop::<T>(|value: &T, window, app| ...)` fires on the hovered eligible element at mouse-up; type-gated by `TypeId`. `gpui::App::stop_propagation()` is callable inside (via the `cx` the listener closure receives — in `cx.listener` form, call `cx.stop_propagation()`).
- `drag_over::<T>(|style, value, window, app| style...)` applies a style refinement while a `T` drag hovers the element.
- `on_drag_move::<T>` fires on EVERY mouse move while a `T` drag is active (not only over the element); `event.bounds` = listening element's bounds, `event.event.position` = pointer. Listener must filter by payload.
- `cx.has_active_drag()` (on `gpui::App`) gates drag-only overlays. The drag value itself is not readable in render — keep per-drag UI state in `App` fields set from drag callbacks.
- `Entity::update(cx, f)` works with `&mut gpui::App` — used to reset App state from an `on_drag` constructor closure (which gets `&mut App`, not a listener context): capture `cx.entity()` before building the element.
- Divider-drag in slice 2 proved gpui activates drags from `simulate_mouse_down` + `simulate_mouse_move(…, MouseButton::Left, …)` + `simulate_mouse_up` in `#[gpui::test]`.
- Slice-2 surfaces: `Workspace` in `src/workspace/mod.rs` (panes as `Vec<(PaneId, Pane)>`, `close_pane`, `split`, `MIN_PANE_RATIO`); tab rendering in `src/workspace/tab_bar.rs` (`render_tab_bar(workspace, pane, changeset, scroll, cx)`, selectors `workspace-tab-{pane}-{index}`); pane rendering in `src/workspace/pane_grid.rs` (`render_pane`, `DraggedDivider`, `EmptyDragPreview`); App methods `activate_workspace_tab(pane, index, cx)` etc. in `src/app.rs`.

---

### Task 1: Workspace state machine — `move_tab` and `split_with_tab`

**Files:**
- Modify: `src/workspace/mod.rs`

- [ ] **Step 1: Extract a tab-removal helper from `close_tab`**

The source-pane side of a move repeats `close_tab`'s preview/active fixups but must keep the removed item. Refactor `Pane` (private) with:

```rust
impl Pane {
    /// Remove the tab at `index`, fixing the preview slot and selecting the
    /// right (or left, at the end) neighbor. Returns the removed item.
    fn remove_tab(&mut self, index: usize) -> Box<dyn WorkspaceItem> {
        let item = self.tabs.remove(index);
        match self.preview {
            Some(preview) if preview == index => self.preview = None,
            Some(preview) if preview > index => self.preview = Some(preview - 1),
            _ => {}
        }
        self.active = if self.tabs.is_empty() {
            None
        } else {
            match self.active {
                Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
                Some(active) if active > index => Some(active - 1),
                other => other,
            }
        };
        item
    }
}
```

Rewrite `Workspace::close_tab` to use it:

```rust
    pub fn close_tab(&mut self, pane: PaneId, index: usize) -> bool {
        let Some(pane) = self.pane_mut(pane) else {
            return false;
        };
        if index >= pane.tabs.len() {
            return false;
        }
        let previous_active = pane.active;
        pane.remove_tab(index);
        previous_active == Some(index)
    }
```

- [ ] **Step 2: Add `move_tab`**

```rust
    /// Move the tab at `from_index` of `from_pane` to `to_index` of `to_pane`.
    ///
    /// Within one pane this is a reorder: preview status travels with the tab
    /// and the moved tab becomes active. Across panes the moved tab arrives
    /// pinned (the preview slot is per-pane; moving is a deliberate act), the
    /// target pane and moved tab activate, and a target tab already holding
    /// the same file absorbs the drag: it activates and the dragged tab
    /// closes. Dragging the last tab out of a pane closes that pane.
    /// `to_index` is clamped to the target strip's length. Returns false (and
    /// changes nothing) for unknown panes or an out-of-range `from_index`.
    pub fn move_tab(
        &mut self,
        from_pane: PaneId,
        from_index: usize,
        to_pane: PaneId,
        to_index: usize,
    ) -> bool {
        if self.pane(to_pane).is_none() {
            return false;
        }
        let Some(source) = self.pane_mut(from_pane) else {
            return false;
        };
        if from_index >= source.tabs.len() {
            return false;
        }

        if from_pane == to_pane {
            let to_index = to_index.min(source.tabs.len() - 1);
            if to_index == from_index {
                return false;
            }
            let was_preview = source.preview == Some(from_index);
            let item = source.remove_tab(from_index);
            source.tabs.insert(to_index, item);
            // remove_tab shifted preview/active for indices right of the
            // removal; re-shift those displaced by the insertion.
            if let Some(preview) = source.preview {
                if preview >= to_index {
                    source.preview = Some(preview + 1);
                }
            }
            if was_preview {
                source.preview = Some(to_index);
            }
            source.active = Some(to_index);
            return true;
        }

        let item = source.remove_tab(from_index);
        let source_emptied = source.tabs.is_empty();

        let target = self
            .pane_mut(to_pane)
            .expect("target pane checked above");
        if let Some(existing) = target.tabs.iter().position(|tab| tab.key() == item.key()) {
            // Merge: the dragged tab closes instead of duplicating.
            target.active = Some(existing);
        } else {
            let to_index = to_index.min(target.tabs.len());
            target.tabs.insert(to_index, item);
            if let Some(preview) = target.preview {
                if preview >= to_index {
                    target.preview = Some(preview + 1);
                }
            }
            target.active = Some(to_index);
        }
        self.active_pane = to_pane;

        if source_emptied {
            self.close_pane(from_pane);
            self.active_pane = to_pane;
        }
        true
    }
```

Note: `close_pane` may reassign `active_pane` (it activates the pane taking the closed slot); the explicit re-assignment after it keeps the drop target active.

Wait — `close_pane` only reassigns `active_pane` when the closed pane WAS active; after a cross-pane move `active_pane` is already `to_pane`, so the second assignment is a harmless no-op guard. Keep it for clarity or drop it; if clippy flags nothing, keep.

`active` index fixup on merge-insert: inserting at `to_index` displaces existing `active >= to_index` — the explicit `target.active = Some(to_index)` overwrites it anyway, so no extra shift logic is needed for `active`; only `preview` needs the shift.

- [ ] **Step 3: Add `split_with_tab`**

```rust
    /// Split `target_pane` in `direction` and move the tab at
    /// (`from_pane`, `from_index`) into the new pane as its only, pinned tab.
    /// The new pane becomes active. Dragging a pane's last tab out closes the
    /// source pane. Returns the new pane's id, or None (changing nothing) on
    /// unknown panes / index.
    pub fn split_with_tab(
        &mut self,
        target_pane: PaneId,
        direction: SplitDirection,
        from_pane: PaneId,
        from_index: usize,
    ) -> Option<PaneId> {
        if self
            .pane(from_pane)
            .is_none_or(|pane| from_index >= pane.tabs.len())
        {
            return None;
        }
        let new_pane = self.split(target_pane, direction)?;
        let moved = self.move_tab(from_pane, from_index, new_pane, 0);
        debug_assert!(moved, "validated source must move");
        Some(new_pane)
    }
```

(`is_none_or` is stable; if the toolchain rejects it use `!self.pane(from_pane).is_some_and(|pane| from_index < pane.tabs.len())`.)

Edge case covered by tests: dragging a pane's ONLY tab to that same pane's edge zone — `split` creates the new pane, `move_tab` empties the source and collapses it; net result is the tab moving into the new pane with the layout ending structurally where it began.

- [ ] **Step 4: Unit tests**

Append inside `mod tests`:

```rust
    #[test]
    fn reorder_moves_a_tab_and_its_preview_status() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_preview(item("c.rs"));
        assert!(ws.move_tab(0, 2, 0, 0));
        assert_eq!(paths(&ws), ["c.rs", "a.rs", "b.rs"]);
        assert!(ws.is_preview(0, 0), "preview status travels with the tab");
        assert_eq!(ws.active_index(0), Some(0), "moved tab is active");
    }

    #[test]
    fn reorder_to_the_right_lands_at_the_target_index() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        assert!(ws.move_tab(0, 0, 0, 2));
        assert_eq!(paths(&ws), ["b.rs", "c.rs", "a.rs"]);
        assert_eq!(ws.active_index(0), Some(2));
    }

    #[test]
    fn reorder_keeps_an_unrelated_preview_index_correct() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        // Move c.rs (index 2) before a.rs (index 0); preview b.rs shifts to 2.
        assert!(ws.move_tab(0, 2, 0, 0));
        assert_eq!(paths(&ws), ["c.rs", "a.rs", "b.rs"]);
        assert!(ws.is_preview(0, 2));
    }

    #[test]
    fn reorder_to_the_same_position_is_a_no_op() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        assert!(!ws.move_tab(0, 1, 0, 1));
        assert_eq!(paths(&ws), ["a.rs", "b.rs"]);
    }

    #[test]
    fn cross_pane_move_arrives_pinned_and_activates_the_target() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        let right = ws.split(0, SplitDirection::Right).expect("split");
        ws.open_pinned(item("c.rs"));
        // Drag the preview tab b.rs from pane 0 into the right pane at slot 0.
        assert!(ws.move_tab(0, 1, right, 0));
        assert_eq!(paths_in(&ws, 0), ["a.rs"]);
        assert_eq!(paths_in(&ws, right), ["b.rs", "c.rs"]);
        assert!(!ws.is_preview(right, 0), "moved preview becomes pinned");
        assert_eq!(ws.active_pane(), right);
        assert_eq!(ws.active_index(right), Some(0));
    }

    #[test]
    fn cross_pane_move_merges_with_an_existing_tab_for_the_same_file() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        let right = ws.split(0, SplitDirection::Right).expect("split");
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("c.rs"));
        ws.activate_pane(0);
        // Drag pane 0's a.rs onto the right pane, which already holds a.rs.
        assert!(ws.move_tab(0, 0, right, 1));
        assert_eq!(paths_in(&ws, 0), ["b.rs"]);
        assert_eq!(paths_in(&ws, right), ["a.rs", "c.rs"], "no duplicate");
        assert_eq!(ws.active_index(right), Some(0), "existing tab activates");
        assert_eq!(ws.active_pane(), right);
    }

    #[test]
    fn dragging_the_last_tab_out_collapses_the_source_pane() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        let right = ws.split(0, SplitDirection::Right).expect("split");
        ws.activate_pane(0);
        assert!(ws.move_tab(0, 0, right, 0));
        assert_eq!(ws.pane_ids(), [right], "emptied source pane collapsed");
        assert_eq!(ws.layout(), &PaneGroup::Pane(right));
        assert_eq!(paths_in(&ws, right), ["a.rs"]);
        assert_eq!(ws.active_pane(), right);
    }

    #[test]
    fn move_tab_refuses_bad_input() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        assert!(!ws.move_tab(0, 5, 0, 0), "source index out of range");
        assert!(!ws.move_tab(9, 0, 0, 0), "unknown source pane");
        assert!(!ws.move_tab(0, 0, 9, 0), "unknown target pane");
        assert_eq!(paths(&ws), ["a.rs"]);
    }

    #[test]
    fn cross_pane_move_clamps_the_target_index() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        let right = ws.split(0, SplitDirection::Right).expect("split");
        ws.activate_pane(0);
        assert!(ws.move_tab(0, 0, right, 99));
        assert_eq!(paths_in(&ws, right), ["a.rs"]);
    }

    #[test]
    fn split_with_tab_moves_the_tab_into_the_new_pane_pinned() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        let below = ws
            .split_with_tab(0, SplitDirection::Down, 0, 1)
            .expect("split with tab");
        assert_eq!(paths_in(&ws, 0), ["a.rs"]);
        assert_eq!(paths_in(&ws, below), ["b.rs"]);
        assert!(!ws.is_preview(below, 0), "moved preview becomes pinned");
        assert_eq!(ws.active_pane(), below);
        match ws.layout() {
            PaneGroup::Axis(axis) => assert_eq!(axis.axis, SplitAxis::Vertical),
            PaneGroup::Pane(_) => panic!("expected an axis root"),
        }
    }

    #[test]
    fn splitting_out_a_panes_only_tab_collapses_the_source() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        let new_pane = ws
            .split_with_tab(0, SplitDirection::Right, 0, 0)
            .expect("split with tab");
        assert_eq!(ws.pane_ids(), [new_pane]);
        assert_eq!(ws.layout(), &PaneGroup::Pane(new_pane));
        assert_eq!(paths_in(&ws, new_pane), ["a.rs"]);
    }

    #[test]
    fn split_with_tab_refuses_bad_input() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        assert_eq!(ws.split_with_tab(0, SplitDirection::Right, 0, 5), None);
        assert_eq!(ws.split_with_tab(9, SplitDirection::Right, 0, 0), None);
        assert_eq!(ws.pane_ids(), [0], "nothing split");
    }
```

- [ ] **Step 5: Run the workspace unit tests**

Run: `cargo test --lib workspace::tests`
Expected: PASS (all prior + 12 new).

- [ ] **Step 6: Commit**

```bash
git add src/workspace/mod.rs
git commit -m "feat(workspace): move_tab and split_with_tab transitions"
```

---

### Task 2: App plumbing and drag state

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the drop-zone field**

App struct (near `file_tree_highlight_path`):

```rust
    /// While a tab drag hovers a pane's edge zone, the pane and the split
    /// direction its half-highlight previews. None when no edge is hovered.
    pub(crate) tab_drop_zone: Option<(crate::workspace::PaneId, crate::workspace::SplitDirection)>,
```

Constructor literal: `tab_drop_zone: None,`.

- [ ] **Step 2: Add the App methods**

Next to the other workspace methods:

```rust
    pub(crate) fn set_tab_drop_zone(
        &mut self,
        zone: Option<(crate::workspace::PaneId, crate::workspace::SplitDirection)>,
        cx: &mut Context<Self>,
    ) {
        if self.tab_drop_zone != zone {
            self.tab_drop_zone = zone;
            cx.notify();
        }
    }

    pub(crate) fn move_workspace_tab(
        &mut self,
        from_pane: crate::workspace::PaneId,
        from_index: usize,
        to_pane: crate::workspace::PaneId,
        to_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.tab_drop_zone = None;
        if self
            .workspace
            .move_tab(from_pane, from_index, to_pane, to_index)
        {
            self.pane_scroll(to_pane).diff.reset();
            if let Some(index) = self.workspace.active_index(to_pane) {
                self.pane_scroll(to_pane).tab_bar.scroll_to_item(index);
            }
        }
        cx.notify();
    }

    pub(crate) fn split_workspace_pane_with_tab(
        &mut self,
        target_pane: crate::workspace::PaneId,
        direction: crate::workspace::SplitDirection,
        from_pane: crate::workspace::PaneId,
        from_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.tab_drop_zone = None;
        if let Some(new_pane) =
            self.workspace
                .split_with_tab(target_pane, direction, from_pane, from_index)
        {
            self.pane_scroll(new_pane).diff.reset();
        }
        cx.notify();
    }
```

(`SplitDirection` needs `PartialEq` — it derives it already. The tuple comparison needs `PaneId: PartialEq` — it is `usize`.)

`move_workspace_tab` resetting the target diff scroll unconditionally on success is deliberate: the target pane shows a different item afterward in every non-merge case, and in the merge case the activated tab may differ too. (Skip micro-optimizing.)

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles (fields/methods unused warnings may appear if built before Task 3 — proceed to Task 3 before judging warnings; `bin/check` runs at slice end).

Note: to keep the tree green per-commit, commit Tasks 2+3 together if `-D warnings` would flag the not-yet-used methods. Prefer: implement Tasks 2 and 3, run `cargo test`, then commit both as one change.

---

### Task 3: Drag-and-drop rendering

**Files:**
- Modify: `src/workspace/tab_bar.rs` (drag source, tab/strip drop targets, drag preview view)
- Modify: `src/workspace/pane_grid.rs` (edge zones + half-pane highlight)

- [ ] **Step 1: Add `DraggedTab` and the drag preview to `tab_bar.rs`**

```rust
/// A tab being dragged: its source pane and strip index.
#[derive(Clone)]
pub(crate) struct DraggedTab {
    pub pane: PaneId,
    pub index: usize,
    pub title: String,
}

/// Cursor-following preview while a tab is dragged.
pub(crate) struct TabDragPreview {
    title: String,
}

impl gpui::Render for TabDragPreview {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(px(TAB_BAR_HEIGHT - 6.))
            .px_3()
            .bg(rgb(TAB_ACTIVE_BG))
            .border_1()
            .border_color(rgb(TAB_ACCENT))
            .rounded(px(3.))
            .text_size(px(TAB_TEXT_SIZE))
            .font_family(FILE_TREE_FONT_FAMILY)
            .text_color(rgb(TAB_DEFAULT_TEXT))
            .opacity(0.9)
            .child(self.title.clone())
    }
}
```

- [ ] **Step 2: Make tabs drag sources and drop targets**

In the tab-building loop, extend the `tab` element chain (after `.on_mouse_down(MouseButton::Middle, ...)`):

```rust
            .on_drag(
                DraggedTab {
                    pane,
                    index,
                    title: title.clone(),
                },
                {
                    let entity = cx.entity();
                    move |drag, _offset, _window, cx| {
                        let title = drag.title.clone();
                        // A fresh drag must not inherit a stale edge-zone
                        // highlight from the previous drag.
                        entity.update(cx, |app, _cx| app.tab_drop_zone = None);
                        cx.new(|_| TabDragPreview { title })
                    }
                },
            )
            .drag_over::<DraggedTab>(|style, _drag, _window, _cx| {
                style.border_l_2().border_color(rgb(TAB_ACCENT))
            })
            .on_drop(cx.listener(
                move |app, drag: &DraggedTab, _window, cx| {
                    cx.stop_propagation();
                    app.move_workspace_tab(drag.pane, drag.index, pane, index, cx);
                },
            ))
```

`title` is already cloned into the label child — add the extra `.clone()` where needed (the loop builds `title` per tab).

- [ ] **Step 3: Make the strip an end-of-row drop target**

On the `strip` element (before tabs are appended):

```rust
        .drag_over::<DraggedTab>(|style, _drag, _window, _cx| style.bg(rgb(0x1d2733)))
        .on_drop(cx.listener(move |app, drag: &DraggedTab, _window, cx| {
            let end = app.workspace.tabs(pane).len();
            app.move_workspace_tab(drag.pane, drag.index, pane, end, cx);
        }))
```

(The tab's own `on_drop` stops propagation, so the strip handler only fires for drops on empty strip space. Dropping a tab on its own strip's empty space moves it to the end — covered by `move_tab` reorder semantics. `end` may exceed the post-removal length; `move_tab` clamps.)

- [ ] **Step 4: Edge zones + half-pane highlight in `pane_grid.rs`**

In `render_pane`, wrap the detail child so the content area can host the
highlight overlay and zone listeners. Replace the
`.child(app.render_file_detail(...))` call with:

```rust
        .child(render_pane_content(app, pane, repo, changeset, cx))
```

and add:

```rust
/// A pane's diff content plus tab-drag edge zones. While a tab drag hovers
/// the left/right/top/bottom band of the content area, the corresponding
/// half of the pane highlights; dropping there splits the pane in that
/// direction with the dragged tab.
const EDGE_ZONE_FRACTION: f32 = 0.25;
const EDGE_HIGHLIGHT_COLOR: u32 = 0x7da4ff;

fn render_pane_content(
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
    let highlight = app
        .tab_drop_zone
        .filter(|(zone_pane, _)| *zone_pane == pane)
        .map(|(_, direction)| direction)
        .filter(|_| cx.has_active_drag());

    div()
        .id(("workspace-pane-content", pane))
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .on_drag_move(cx.listener(
            move |app, event: &DragMoveEvent<DraggedTab>, _window, cx| {
                let bounds = event.bounds;
                let position = event.event.position;
                if !bounds.contains(&position) {
                    // Only clear a zone this pane owns; other panes manage theirs.
                    if app.tab_drop_zone.is_some_and(|(zone_pane, _)| zone_pane == pane) {
                        app.set_tab_drop_zone(None, cx);
                    }
                    return;
                }
                let x = (position.x - bounds.left()) / bounds.size.width;
                let y = (position.y - bounds.top()) / bounds.size.height;
                let zone = if x < EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Left)
                } else if x > 1. - EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Right)
                } else if y < EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Up)
                } else if y > 1. - EDGE_ZONE_FRACTION {
                    Some(SplitDirection::Down)
                } else {
                    None
                };
                app.set_tab_drop_zone(zone.map(|direction| (pane, direction)), cx);
            },
        ))
        .on_drop(cx.listener(move |app, drag: &DraggedTab, _window, cx| {
            let zone = app
                .tab_drop_zone
                .filter(|(zone_pane, _)| *zone_pane == pane);
            if let Some((_, direction)) = zone {
                app.split_workspace_pane_with_tab(pane, direction, drag.pane, drag.index, cx);
            } else {
                app.set_tab_drop_zone(None, cx);
            }
        }))
        .child(app.render_file_detail(repo, changeset, active_path.as_deref(), &scrolls.diff))
        .when_some(highlight, |content, direction| {
            let selector = format!("workspace-drop-half-{pane}");
            content.child(
                div()
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .bg(rgb(EDGE_HIGHLIGHT_COLOR))
                    .opacity(0.18)
                    .map(|half| match direction {
                        SplitDirection::Left => half.left_0().top_0().bottom_0().w(relative(0.5)),
                        SplitDirection::Right => half.right_0().top_0().bottom_0().w(relative(0.5)),
                        SplitDirection::Up => half.top_0().left_0().right_0().h(relative(0.5)),
                        SplitDirection::Down => half.bottom_0().left_0().right_0().h(relative(0.5)),
                    }),
            )
        })
        .into_any_element()
}
```

Imports: `SplitDirection`, `super::tab_bar::DraggedTab`. `Bounds::contains(&point)` exists in gpui. If `.w(relative(0.5))` rejects the type (`w` takes `impl Into<Length>`, `relative` returns `DefiniteLength` — `Length: From<DefiniteLength>` exists, so it compiles), fall back to `w_1_2()` / `h_1_2()` if available.

The original `render_pane` keeps the tab bar child; only the detail child moves into `render_pane_content`.

- [ ] **Step 5: Run the existing suite**

Run: `cargo test`
Expected: PASS (no behavior change without a drag).

- [ ] **Step 6: Commit Tasks 2+3 together**

```bash
git add src/app.rs src/workspace/tab_bar.rs src/workspace/pane_grid.rs
git commit -m "feat(workspace): tab drag and drop with edge-zone splits"
```

---

### Task 4: View tests

**Files:**
- Modify: `src/workspace/pane_grid.rs` (drag interaction tests; reuse its fixtures)

- [ ] **Step 1: Add a drag helper**

In `pane_grid.rs` tests:

```rust
    /// Drag from `start` to `end` with the left button: down, an activation
    /// nudge past gpui's 2px threshold, the real move, then up.
    fn simulate_drag(visual: &mut VisualTestContext, start: Point<Pixels>, end: Point<Pixels>) {
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_move(
            start + point(px(4.), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    }
```

(Imports: `Pixels`, `Point` — extend the existing `use gpui::{...}`.)

- [ ] **Step 2: The four interaction tests**

```rust
    #[gpui::test]
    async fn dragging_a_tab_within_its_strip_reorders(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        // Pin alpha, pin beta: strip order [alpha, beta].
        let row = visual
            .debug_bounds("changed-file-row-2")
            .expect("alpha row");
        crate::workspace::test_util::simulate_double_click(&mut visual, row.center());
        let row = visual
            .debug_bounds("changed-file-row-1")
            .expect("beta row");
        crate::workspace::test_util::simulate_double_click(&mut visual, row.center());
        cx.run_until_parked();

        let tab0 = visual.debug_bounds("workspace-tab-0-0").expect("tab 0");
        let tab1 = visual.debug_bounds("workspace-tab-0-1").expect("tab 1");
        // Drop beta onto alpha: insert before index 0.
        simulate_drag(&mut visual, tab1.center(), tab0.center());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let paths: Vec<String> = app
                    .workspace
                    .tabs(0)
                    .iter()
                    .map(|tab| tab.path().to_string())
                    .collect();
                assert_eq!(paths, ["nested/beta.txt", "alpha.txt"]);
            })
            .expect("read reordered tabs");
    }

    #[gpui::test]
    async fn dragging_a_tab_to_another_pane_moves_it_pinned(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        // Pin alpha and preview beta in pane 0, then split.
        let row = visual
            .debug_bounds("changed-file-row-2")
            .expect("alpha row");
        crate::workspace::test_util::simulate_double_click(&mut visual, row.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        // Drag the preview tab (beta, index 1) onto pane 1's empty strip.
        let tab = visual.debug_bounds("workspace-tab-0-1").expect("beta tab");
        let strip = visual
            .debug_bounds("workspace-tab-bar-1")
            .expect("pane 1 tab bar");
        simulate_drag(&mut visual, tab.center(), strip.center());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1);
                assert_eq!(app.workspace.tabs(1).len(), 1);
                assert!(
                    !app.workspace.is_preview(1, 0),
                    "moved preview arrives pinned"
                );
                assert_eq!(app.workspace.active_pane(), 1);
            })
            .expect("read moved tab");
    }

    #[gpui::test]
    async fn dragging_a_tab_to_an_edge_zone_splits_the_pane(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        let row = visual
            .debug_bounds("changed-file-row-2")
            .expect("alpha row");
        crate::workspace::test_util::simulate_double_click(&mut visual, row.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Drag beta to the right edge band of pane 0's content.
        let tab = visual.debug_bounds("workspace-tab-0-1").expect("beta tab");
        let pane = visual.debug_bounds("workspace-pane-0").expect("pane 0");
        let target = gpui::Point {
            x: pane.right() - px(10.),
            y: pane.center().y,
        };
        simulate_drag(&mut visual, tab.center(), target);
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids().len(), 2, "edge drop split");
                let new_pane = app.workspace.pane_ids()[1];
                assert_eq!(app.workspace.tabs(0).len(), 1);
                assert_eq!(app.workspace.tabs(new_pane).len(), 1);
                assert!(!app.workspace.is_preview(new_pane, 0));
                assert_eq!(app.workspace.active_pane(), new_pane);
            })
            .expect("read split-by-drop");
    }

    #[gpui::test]
    async fn dragging_the_last_tab_out_collapses_the_pane(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);
        // One pinned tab in pane 0; split right (pane 1 active+empty).
        let row = visual
            .debug_bounds("changed-file-row-2")
            .expect("alpha row");
        crate::workspace::test_util::simulate_double_click(&mut visual, row.center());
        cx.run_until_parked();
        let split = visual
            .debug_bounds("workspace-split-right-0")
            .expect("split control");
        visual.simulate_click(split.center(), Modifiers::none());
        cx.run_until_parked();

        // Drag pane 0's only tab into pane 1's strip: pane 0 collapses.
        let tab = visual.debug_bounds("workspace-tab-0-0").expect("alpha tab");
        let strip = visual
            .debug_bounds("workspace-tab-bar-1")
            .expect("pane 1 tab bar");
        simulate_drag(&mut visual, tab.center(), strip.center());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids(), [1], "source pane collapsed");
                assert_eq!(app.workspace.tabs(1).len(), 1);
            })
            .expect("read collapsed layout");
    }
```

Note on the cross-pane test's drop point: pane 1's strip is empty, so `workspace-tab-bar-1`'s center is inside the strip (the corner controls sit at the right edge); if the drop lands on a corner control instead, aim at `strip.left() + px(40.)`.

- [ ] **Step 3: Run them**

Run: `cargo test --lib workspace::pane_grid`
Expected: PASS. If a drop does not register, debug with the divider-drag test as the known-good reference (drag activation) before touching production code.

- [ ] **Step 4: Commit**

```bash
git add src/workspace/pane_grid.rs
git commit -m "test(workspace): view coverage for tab drag and drop"
```

---

### Task 5: Spec update

**Files:**
- Modify: `docs/specs/review/workflow.md`

- [ ] **Step 1: Add a drag-and-drop section** after "Splitting the diff area into panes":

```markdown
## Rearranging tabs by dragging

Tabs answer to the mouse: a reviewer can drag one along its own row to reorder, drop it on another pane's tab row to move it, or drop it on the edge of a pane's content to carve out a new split — the same gestures Zed and VS Code train.

**Triggering conditions**

- The user drags a tab and drops it on a tab row, on another tab, or on the edge zone of a pane's content area.

**Observable outcomes**

- While a tab is dragged, a floating preview of the tab follows the cursor.
- Dropping a tab on another tab inserts it at that position; dropping it on empty tab-row space appends it at the end. An insertion indicator marks the target while hovering. Reordering within a pane keeps the tab's preview or pinned status.
- Dropping a tab on a different pane's tab row moves it there. The moved tab arrives pinned — even if it was the preview tab — and becomes the active tab of the now-active target pane.
- If the target pane already holds a tab for the same file, the drop merges: the existing tab activates and the dragged tab closes rather than duplicating.
- Dragging a tab over the left, right, top, or bottom band of a pane's content area highlights the corresponding half of the pane; dropping there splits that pane in that direction, and the dragged tab becomes the new pane's only, pinned tab.
- Dragging the last tab out of a pane closes that pane; the layout collapses and returns its space to siblings.

**Edge cases**

- Dropping a tab back where it started changes nothing.
- Dragging a pane's only tab to that same pane's edge zone moves the tab into the new half; no empty pane is left behind.
- Releasing a drag outside any drop target leaves every tab where it was.
```

- [ ] **Step 2: Commit**

```bash
git add docs/specs/review/workflow.md
git commit -m "docs(spec): tab drag and drop in the review workflow"
```

---

### Task 6: Final verification

- [ ] **Step 1:** `bin/check` — zero warnings/failures (no `#[allow]` without user approval).
- [ ] **Step 2:** Re-read the design's "Drag and Drop" section; each sentence must map to a landed test. Layout persistence must NOT appear.
- [ ] **Step 3:** Commit fixes if any.

---

## Self-Review

- **Spec coverage:** reorder + indicator ✓ (T1/T3/T4), cross-pane move + pinned + merge ✓ (T1/T3/T4), edge-zone split + half highlight ✓ (T1/T3/T4), last-tab-out collapse ✓ (T1/T4), spec doc ✓ (T5).
- **Type consistency:** `move_tab(PaneId, usize, PaneId, usize) -> bool`, `split_with_tab(PaneId, SplitDirection, PaneId, usize) -> Option<PaneId>`, `DraggedTab { pane, index, title }` used consistently.
- **Known risks:** (a) drop dispatch ordering tab-vs-strip relies on child-first bubble with `stop_propagation`; if both fire, the tab handler runs first and the strip's second `move_tab` no-ops on a stale index — verify in T4 and guard if needed. (b) `drag_over` border indicator shifts tab width by 2px while hovered; acceptable, matches a visible indicator requirement.
