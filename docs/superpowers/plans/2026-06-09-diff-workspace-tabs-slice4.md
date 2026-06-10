# Diff Workspace Tabs — Slice 4 Implementation Plan (Layout Persistence)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The pane-tree shape (split axes, ratios, active pane) persists per repository in the settings store. Tabs are never persisted. A corrupt saved layout falls back silently to a single pane and is overwritten on the next save.

**Architecture:** A serializable `SavedPaneGroup` mirror of the layout tree lives in `src/workspace/mod.rs` (serde only, still gpui-free) with `Workspace::saved_layout()` / `Workspace::from_saved()` conversions; `from_saved` validates structurally and falls back to `Workspace::new()`. `Settings` gains a `workspace_layouts: BTreeMap<PathBuf, SavedPaneGroup>` keyed by repository path. `App` saves at every structural mutation (split, close pane, pane activation, tab move/split-with-tab, divider release) and restores in `apply_open_repository`.

**Tech Stack:** Rust, serde/serde_json (already dependencies). Verification: `bin/check`.

---

## Facts and constraints

- `Settings` currently derives `Eq`; `f32` ratios are not `Eq` → drop `Eq` from `Settings` (tests only need `PartialEq` via `assert_eq!`). `RecentRepository` keeps `Eq`.
- `Settings` is `#[serde(default)]`, so the new field loads as empty from older files. A field-level corrupt value fails the WHOLE settings parse (load returns defaults) — acceptable: that path already exists for any corrupt settings file. Semantic corruption (bad ratios, no panes) is validated in `from_saved`.
- Divider drags have no end callback, but a mouse-up over the axis container delivers `on_drop::<DraggedDivider>` — use it as the persist point for ratios. A release outside the container persists at the next structural change or changeset transition instead (documented gap).
- `pane_scrolls` is keyed by `PaneId`; replacing the workspace on repo open restarts ids at 0 → clear the map there.
- Existing repo-open test pattern with a settings store: `new_with_settings_store_path` (`src/app.rs`, `#[cfg(test)]`), used by `failed_recent_repository_activation_persists_unavailable_state`.
- Layouts map growth is bounded by pruning to the recent-repositories list inside `record_recent_repository`.

---

### Task 1: Saved layout types + conversions + validation (workspace)

**Files:**
- Modify: `src/workspace/mod.rs`

- [ ] **Step 1: Types**

```rust
use serde::{Deserialize, Serialize};

/// Serializable shape of the pane tree: structure, ratios, and which pane is
/// active — never tabs (tabs are per-changeset by definition).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SavedPaneGroup {
    Pane {
        active: bool,
    },
    Split {
        axis: SavedSplitAxis,
        ratios: Vec<f32>,
        children: Vec<SavedPaneGroup>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedSplitAxis {
    Horizontal,
    Vertical,
}
```

- [ ] **Step 2: `saved_layout`**

```rust
    /// Snapshot the layout for persistence.
    pub fn saved_layout(&self) -> SavedPaneGroup {
        Self::save_node(&self.layout, self.active_pane)
    }

    fn save_node(node: &PaneGroup, active_pane: PaneId) -> SavedPaneGroup {
        match node {
            PaneGroup::Pane(id) => SavedPaneGroup::Pane {
                active: *id == active_pane,
            },
            PaneGroup::Axis(axis) => SavedPaneGroup::Split {
                axis: match axis.axis {
                    SplitAxis::Horizontal => SavedSplitAxis::Horizontal,
                    SplitAxis::Vertical => SavedSplitAxis::Vertical,
                },
                ratios: axis.ratios.clone(),
                children: axis.children.iter().map(|child| Self::save_node(child, active_pane)).collect(),
            },
        }
    }
```

- [ ] **Step 3: `from_saved` with validation**

```rust
    /// Rebuild a workspace (all panes empty) from a saved layout. A layout
    /// that fails validation — no panes, ratio/child count mismatch,
    /// non-finite or non-positive ratios, fewer than two children under an
    /// axis, or not exactly one active pane — falls back silently to a
    /// single pane, to be overwritten on the next save.
    pub fn from_saved(saved: &SavedPaneGroup) -> Workspace {
        if !Self::saved_is_valid(saved) {
            return Workspace::new();
        }
        let mut panes = Vec::new();
        let mut active_pane = None;
        let mut next_id = 0;
        let layout = Self::restore_node(saved, &mut panes, &mut active_pane, &mut next_id);
        let Some(active) = active_pane else {
            return Workspace::new();
        };
        Workspace {
            panes,
            layout,
            active_pane: active,
            next_id,
        }
    }

    fn saved_is_valid(saved: &SavedPaneGroup) -> bool {
        fn check(node: &SavedPaneGroup, active_count: &mut usize) -> bool {
            match node {
                SavedPaneGroup::Pane { active } => {
                    if *active {
                        *active_count += 1;
                    }
                    true
                }
                SavedPaneGroup::Split {
                    ratios, children, ..
                } => {
                    children.len() >= 2
                        && ratios.len() == children.len()
                        && ratios.iter().all(|ratio| ratio.is_finite() && *ratio > 0.)
                        && children.iter().all(|child| check(child, active_count))
                }
            }
        }
        let mut active_count = 0;
        check(saved, &mut active_count) && active_count == 1
    }

    fn restore_node(
        saved: &SavedPaneGroup,
        panes: &mut Vec<(PaneId, Pane)>,
        active_pane: &mut Option<PaneId>,
        next_id: &mut usize,
    ) -> PaneGroup {
        match saved {
            SavedPaneGroup::Pane { active } => {
                let id = *next_id;
                *next_id += 1;
                panes.push((id, Pane::new()));
                if *active {
                    *active_pane = Some(id);
                }
                PaneGroup::Pane(id)
            }
            SavedPaneGroup::Split {
                axis,
                ratios,
                children,
            } => {
                let id = *next_id;
                *next_id += 1;
                let total: f32 = ratios.iter().sum();
                PaneGroup::Axis(AxisNode {
                    id,
                    axis: match axis {
                        SavedSplitAxis::Horizontal => SplitAxis::Horizontal,
                        SavedSplitAxis::Vertical => SplitAxis::Vertical,
                    },
                    ratios: ratios.iter().map(|ratio| ratio / total).collect(),
                    children: children
                        .iter()
                        .map(|child| Self::restore_node(child, panes, active_pane, next_id))
                        .collect(),
                })
            }
        }
    }
```

- [ ] **Step 4: Unit tests** — round trip single pane / nested split with resized ratios + active pane; ratio normalization on load; each corrupt case (zero children, one child, count mismatch, NaN ratio, zero ratio, no active, two actives) falls back to `Workspace::new()` shape; serde JSON round trip of `SavedPaneGroup`.

- [ ] **Step 5:** `cargo test --lib workspace::tests` → PASS. Commit: `feat(workspace): serializable layout snapshots with corrupt fallback`.

---

### Task 2: Settings field

**Files:**
- Modify: `src/settings.rs`

- [ ] **Step 1:** Drop `Eq` from `Settings`'s derive; add the field:

```rust
    /// Per-repository workspace pane layouts (split shape only; never tabs).
    pub workspace_layouts: BTreeMap<PathBuf, crate::workspace::SavedPaneGroup>,
```

with `use std::collections::BTreeMap;` and the construction sites updated (`Settings { recent_repositories, .. }` literals gain `workspace_layouts: BTreeMap::new()` or use `..Default::default()`).

- [ ] **Step 2:** Tests: a save/load round trip including one layout entry; the existing default/malformed tests still pass.
- [ ] **Step 3:** `cargo test --lib settings` → PASS. Commit: `feat(settings): persist per-repository workspace layouts`.

---

### Task 3: App integration

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Persist helper**

```rust
    /// Record the current pane layout for the open repository and write
    /// settings. Called on every structural workspace change.
    fn persist_workspace_layout(&mut self) {
        let Mode::RepoOpen { repo } = &self.mode else {
            return;
        };
        self.settings
            .workspace_layouts
            .insert(repo.path.clone(), self.workspace.saved_layout());
        self.persist_settings();
    }
```

- [ ] **Step 2: Call sites** — at the end of the mutating branch (inside the `if changed` arms) of: `activate_workspace_pane`, `split_workspace_pane`, `close_workspace_pane`, `move_workspace_tab` (layout may collapse + active pane changes), `split_workspace_pane_with_tab`. NOT in `resize_workspace_divider` (fires per mouse-move); instead persist ratios on divider release via `on_drop::<DraggedDivider>` on the axis container in `pane_grid.rs`:

```rust
        .on_drop(cx.listener(move |app, _drag: &DraggedDivider, _window, cx| {
            app.persist_workspace_layout();
            cx.notify();
        }))
```

(`persist_workspace_layout` becomes `pub(crate)`.) Also persist in `close_changeset` and `open_changeset` (cheap catch-all for ratio drags released off-target).

- [ ] **Step 3: Restore on repo open** — in `apply_open_repository`, before `self.mode = Mode::RepoOpen { repo }`: persist the outgoing repo's layout (`self.persist_workspace_layout()`); after setting the new mode/repo path: replace the workspace and scroll map:

```rust
        self.workspace = match self.settings.workspace_layouts.get(&recent_path) {
            Some(saved) => crate::workspace::Workspace::from_saved(saved),
            None => crate::workspace::Workspace::new(),
        };
        self.pane_scrolls.borrow_mut().clear();
```

(`recent_path` is the repo path captured at the top of the function.)

- [ ] **Step 4: Prune with recents** — in `record_recent_repository`, after `truncate`:

```rust
        let kept: std::collections::BTreeSet<_> =
            recents.iter().map(|recent| recent.path.clone()).collect();
        self.settings
            .workspace_layouts
            .retain(|path, _| kept.contains(path));
```

- [ ] **Step 5: Tests** (app-level, `#[gpui::test]` in `src/app.rs` or `pane_grid.rs` following existing settings-store tests):
  1. `pane_layout_persists_across_repository_reopen`: settings store in a tempdir; open repo, enter changeset, split right, drag-resize not needed; construct a SECOND window/App with the same store path; open the same repo; assert `workspace.pane_ids().len() == 2`, panes empty, active pane restored.
  2. `corrupt_saved_layout_falls_back_to_a_single_pane`: write a settings.json whose layout entry is semantically corrupt (e.g. `"ratios": [0.5]` with two children); open the repo; assert single pane and no panic.

- [ ] **Step 6:** `cargo test` → PASS. Commit: `feat(workspace): restore per-repository pane layouts`.

---

### Task 4: Spec + verification

- [ ] **Step 1:** `docs/specs/review/workflow.md` — in "Splitting the diff area into panes", replace the within-a-session persistence bullet with cross-session, per-repository wording and add corrupt-fallback edge case:

Replace:

```markdown
- The split arrangement survives leaving and re-entering changesets within a session; the panes come back empty because tabs belong to a single changeset.
```

with:

```markdown
- The split arrangement — axes, proportions, and which pane was active — is remembered per repository: re-entering any changeset, or reopening the repository later, restores it. The panes always come back empty because tabs belong to a single changeset.
```

and add to **Edge cases**:

```markdown
- A saved layout that cannot be read is discarded silently: the workspace opens as a single pane and the stored layout is replaced on the next change.
```

- [ ] **Step 2:** Commit: `docs(spec): per-repository pane layout persistence`.
- [ ] **Step 3:** `bin/check` → clean. Re-read the design's "Layout Persistence" section; every sentence maps to a test. Commit fixes if any.

---

## Self-Review

- Per-repo shape+ratios+active persisted ✓ (T1/T2/T3); tabs never persisted ✓ (SavedPaneGroup has no item field; restore creates empty panes); corrupt → silent single pane ✓ (T1 validation + T3 test); overwrite on next save ✓ (insert on every structural change).
- Known gap: ratios changed by a divider drag released outside the axis container persist at the next structural change or changeset open/close rather than immediately — documented above, invisible in practice.
