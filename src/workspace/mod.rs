//! Tabbed workspace state for the changeset review screen.
//!
//! Implements slices 1-2 of `docs/superpowers/specs/2026-06-09-diff-workspace-tabs-design.md`:
//! a tree of panes (axis nodes with ratio-weighted children, pane leaves),
//! each pane holding preview and pinned tabs, with exactly one pane active.
//! Later slices add drag and drop and layout persistence on top of this
//! module without changing its public API. All state transitions here are
//! pure and unit-tested without gpui; rendering lives in `pane_grid` and
//! `tab_bar`.

pub mod pane_grid;
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
    /// A fresh tab holding the same content; used when splitting a pane
    /// copies its active item into the new pane.
    fn clone_item(&self) -> Box<dyn WorkspaceItem>;
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

    fn clone_item(&self) -> Box<dyn WorkspaceItem> {
        Box::new(FileDiffItem::new(self.path.clone()))
    }
}

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

pub struct Workspace {
    panes: Vec<(PaneId, Pane)>,
    layout: PaneGroup,
    active_pane: PaneId,
    /// Shared id counter for panes and axis nodes.
    next_id: usize,
}

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
        self.panes
            .iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, pane)| pane)
    }

    fn pane_mut(&mut self, pane: PaneId) -> Option<&mut Pane> {
        self.panes
            .iter_mut()
            .find(|(id, _)| *id == pane)
            .map(|(_, pane)| pane)
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

    /// Every open tab as `(pane, key)`, across all panes. Lets callers with
    /// per-tab state (e.g. diff selections) prune entries for tabs that no
    /// longer exist without threading pane/tab bookkeeping through every
    /// mutation themselves.
    pub(crate) fn open_keys(&self) -> Vec<(PaneId, String)> {
        self.panes
            .iter()
            .flat_map(|(pane_id, pane)| {
                pane.tabs
                    .iter()
                    .map(move |tab| (*pane_id, tab.key().to_string()))
            })
            .collect()
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
        self.pane(pane)
            .map(|pane| pane.tabs.as_slice())
            .unwrap_or(&[])
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
        self.pane(pane)
            .is_some_and(|pane| pane.preview == Some(index))
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
    /// end of the strip) becomes active. Closing a pane's last tab closes the
    /// pane itself — unless it is the only remaining pane, which stays and
    /// shows the placeholder. Returns true when the shown content changed.
    pub fn close_tab(&mut self, pane: PaneId, index: usize) -> bool {
        let pane_id = pane;
        let Some(pane) = self.pane_mut(pane_id) else {
            return false;
        };
        if index >= pane.tabs.len() {
            return false;
        }
        let previous_active = pane.active;
        pane.remove_tab(index);
        if pane.tabs.is_empty() {
            // Refused for the last remaining pane, which keeps the placeholder.
            self.close_pane(pane_id);
        }
        previous_active == Some(index)
    }

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

        let target = self.pane_mut(to_pane).expect("target pane checked above");
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
            // close_pane only reassigns active_pane when closing the active
            // pane, and the target is active here, so this cannot undo the
            // activation above.
            self.close_pane(from_pane);
        }
        true
    }

    /// Split `pane` and open a copy of its active item in the new pane,
    /// preserving the item's preview/pinned status. The new pane starts
    /// empty when `pane` shows no item. Returns the new pane's id, or None
    /// when `pane` does not exist.
    pub fn split_with_active_item(
        &mut self,
        pane: PaneId,
        direction: SplitDirection,
    ) -> Option<PaneId> {
        let cloned = self.pane(pane).and_then(|pane| {
            let index = pane.active?;
            let item = pane.tabs.get(index)?;
            Some((item.clone_item(), pane.preview == Some(index)))
        });
        let new_pane = self.split(pane, direction)?;
        // The new pane is active, so the open lands there.
        if let Some((item, preview)) = cloned {
            if preview {
                self.open_preview(item);
            } else {
                self.open_pinned(item);
            }
        }
        Some(new_pane)
    }

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

    /// Cycle the active pane's active tab forward, wrapping at the end.
    /// Returns true when the pane's shown content changed.
    pub fn activate_next_tab(&mut self) -> bool {
        self.cycle_tab(1)
    }

    /// Cycle the active pane's active tab backward, wrapping at the start.
    /// Returns true when the pane's shown content changed.
    pub fn activate_previous_tab(&mut self) -> bool {
        self.cycle_tab(-1)
    }

    fn cycle_tab(&mut self, step: isize) -> bool {
        let pane_id = self.active_pane;
        let Some(pane) = self.pane_mut(pane_id) else {
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
        if !Self::split_in_node(
            &mut self.layout,
            pane,
            direction,
            new_pane,
            &mut self.next_id,
        ) {
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
                    let target_position = axis
                        .children
                        .iter()
                        .position(|child| matches!(child, PaneGroup::Pane(id) if *id == target));
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
                axis.children
                    .iter_mut()
                    .any(|child| Self::split_in_node(child, target, direction, new_pane, next_id))
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

    fn resize_in_node(node: &mut PaneGroup, axis_id: usize, divider: usize, fraction: f32) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str) -> Box<dyn WorkspaceItem> {
        Box::new(FileDiffItem::new(path.to_string()))
    }

    fn pane(ws: &Workspace) -> PaneId {
        ws.active_pane()
    }

    fn paths(workspace: &Workspace) -> Vec<&str> {
        paths_in(workspace, workspace.active_pane())
    }

    fn paths_in(workspace: &Workspace, pane: PaneId) -> Vec<&str> {
        workspace.tabs(pane).iter().map(|tab| tab.path()).collect()
    }

    fn layout_ratios(ws: &Workspace) -> Vec<f32> {
        match ws.layout() {
            PaneGroup::Axis(axis) => axis.ratios.clone(),
            PaneGroup::Pane(_) => panic!("expected an axis root"),
        }
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
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
        assert!(ws.is_preview(pane(&ws), 0));
    }

    #[test]
    fn second_preview_replaces_the_first_in_place() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(ws.open_preview(item("b.rs")));
        assert_eq!(paths(&ws), ["b.rs"]);
        assert!(ws.is_preview(pane(&ws), 0));
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
    }

    #[test]
    fn preview_keeps_its_strip_position_when_replaced() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("pinned.rs"));
        ws.open_preview(item("a.rs"));
        ws.activate_tab(pane(&ws), 0);
        ws.open_preview(item("b.rs"));
        assert_eq!(paths(&ws), ["pinned.rs", "b.rs"]);
        assert!(ws.is_preview(pane(&ws), 1));
        assert_eq!(ws.active_index(pane(&ws)), Some(1));
    }

    #[test]
    fn opening_an_already_open_file_activates_instead_of_duplicating() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        assert!(ws.open_preview(item("a.rs")));
        assert_eq!(paths(&ws), ["a.rs", "b.rs"]);
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
        assert!(!ws.is_preview(pane(&ws), 0), "pinned tab must stay pinned");
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
        assert!(ws.is_preview(pane(&ws), 0));
        assert!(!ws.is_preview(pane(&ws), 1));
        assert_eq!(ws.active_index(pane(&ws)), Some(1));
    }

    #[test]
    fn open_pinned_promotes_an_existing_preview_tab_in_place() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(
            !ws.open_pinned(item("a.rs")),
            "already active: no content change"
        );
        assert_eq!(paths(&ws), ["a.rs"]);
        assert!(
            !ws.is_preview(pane(&ws), 0),
            "double-open must pin the preview tab"
        );
    }

    #[test]
    fn activate_tab_out_of_range_is_a_no_op() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(!ws.activate_tab(pane(&ws), 5));
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
    }

    #[test]
    fn closing_the_active_tab_activates_the_right_neighbor() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        ws.activate_tab(pane(&ws), 1);
        assert!(ws.close_tab(pane(&ws), 1));
        assert_eq!(paths(&ws), ["a.rs", "c.rs"]);
        assert_eq!(
            ws.active_index(pane(&ws)),
            Some(1),
            "right neighbor (c.rs) becomes active"
        );
    }

    #[test]
    fn closing_the_last_tab_in_the_strip_activates_the_left_neighbor() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        assert!(ws.close_tab(pane(&ws), 1));
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_item() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        assert!(!ws.close_tab(pane(&ws), 0), "active content unchanged");
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
        assert_eq!(ws.active_item(pane(&ws)).unwrap().path(), "b.rs");
    }

    #[test]
    fn closing_the_only_tab_of_the_only_pane_keeps_the_pane() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(ws.close_tab(pane(&ws), 0));
        assert_eq!(ws.pane_ids(), [0], "the last pane is never auto-closed");
        assert!(ws.tabs(pane(&ws)).is_empty());
        assert_eq!(ws.active_index(pane(&ws)), None);
        assert!(ws.active_item(pane(&ws)).is_none());
    }

    #[test]
    fn closing_a_panes_last_tab_closes_the_pane() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        let right = ws.split(0, SplitDirection::Right).expect("split");
        ws.open_pinned(item("b.rs"));
        assert!(ws.close_tab(right, 0));
        assert_eq!(ws.pane_ids(), [0], "emptied pane collapsed");
        assert_eq!(ws.layout(), &PaneGroup::Pane(0));
        assert_eq!(ws.active_pane(), 0);
        assert_eq!(paths_in(&ws, 0), ["a.rs"]);
    }

    #[test]
    fn closing_an_inactive_panes_last_tab_keeps_the_active_pane() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        let right = ws.split(0, SplitDirection::Right).expect("split");
        ws.open_pinned(item("b.rs"));
        ws.activate_pane(0);
        assert!(ws.close_tab(right, 0));
        assert_eq!(ws.pane_ids(), [0]);
        assert_eq!(ws.active_pane(), 0, "active pane is untouched");
    }

    #[test]
    fn closing_the_preview_tab_clears_the_preview_slot() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.close_tab(pane(&ws), 1);
        ws.open_preview(item("c.rs"));
        assert_eq!(paths(&ws), ["a.rs", "c.rs"], "a fresh preview tab appends");
        assert!(ws.is_preview(pane(&ws), 1));
    }

    #[test]
    fn closing_a_tab_left_of_the_preview_shifts_the_preview_index() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.close_tab(pane(&ws), 0);
        assert!(
            ws.is_preview(pane(&ws), 0),
            "preview index follows the shifted tab"
        );
        assert_eq!(ws.active_item(pane(&ws)).unwrap().path(), "b.rs");
    }

    #[test]
    fn closing_a_tab_left_of_both_preview_and_active_shifts_both_indices() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_preview(item("c.rs"));
        assert!(
            !ws.close_tab(pane(&ws), 0),
            "active content unchanged by closing a left tab"
        );
        assert_eq!(paths(&ws), ["b.rs", "c.rs"]);
        assert!(
            ws.is_preview(pane(&ws), 1),
            "preview index shifts with its tab"
        );
        assert_eq!(ws.active_index(pane(&ws)), Some(1));
        assert_eq!(ws.active_item(pane(&ws)).unwrap().path(), "c.rs");
    }

    #[test]
    fn promote_tab_pins_only_the_preview_tab() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        ws.promote_tab(pane(&ws), 0);
        assert!(!ws.is_preview(pane(&ws), 0));
        ws.open_preview(item("b.rs"));
        assert_eq!(
            paths(&ws),
            ["a.rs", "b.rs"],
            "promoted tab no longer absorbs previews"
        );
    }

    #[test]
    fn clear_closes_everything() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.clear();
        assert!(ws.tabs(pane(&ws)).is_empty());
        assert_eq!(ws.active_index(pane(&ws)), None);
        ws.open_preview(item("c.rs"));
        assert!(ws.is_preview(pane(&ws), 0));
    }

    #[test]
    fn close_tab_out_of_range_is_a_no_op() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        assert!(!ws.close_tab(pane(&ws), 7));
        assert_eq!(paths(&ws), ["a.rs"]);
    }

    #[test]
    fn closing_a_tab_right_of_active_and_preview_shifts_nothing() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        ws.activate_tab(pane(&ws), 0);
        assert!(!ws.close_tab(pane(&ws), 2), "active content unchanged");
        assert_eq!(paths(&ws), ["a.rs", "b.rs"]);
        assert_eq!(ws.active_index(pane(&ws)), Some(0));
        assert!(
            ws.is_preview(pane(&ws), 1),
            "preview left of the closed tab keeps its index"
        );
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
        assert_eq!(
            nested.children,
            vec![PaneGroup::Pane(right), PaneGroup::Pane(below)]
        );
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
        assert_eq!(
            paths_in(&ws, new_pane),
            ["a.rs"],
            "no duplicate within a pane"
        );
    }

    #[test]
    fn split_with_active_item_copies_the_pinned_item_pinned() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        let right = ws
            .split_with_active_item(0, SplitDirection::Right)
            .expect("split");
        assert_eq!(paths_in(&ws, 0), ["a.rs", "b.rs"], "source pane untouched");
        assert_eq!(paths_in(&ws, right), ["b.rs"], "active item copied");
        assert!(!ws.is_preview(right, 0), "pinned source stays pinned");
        assert_eq!(ws.active_pane(), right);
        assert_eq!(ws.active_index(right), Some(0));
    }

    #[test]
    fn split_with_active_item_preserves_preview_status() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        let right = ws
            .split_with_active_item(0, SplitDirection::Right)
            .expect("split");
        assert_eq!(paths_in(&ws, right), ["a.rs"]);
        assert!(ws.is_preview(right, 0), "preview source splits as preview");
        // The copy occupies the new pane's preview slot: browsing on keeps
        // replacing in place.
        ws.open_preview(item("b.rs"));
        assert_eq!(paths_in(&ws, right), ["b.rs"]);
        assert!(ws.is_preview(0, 0), "source pane's preview is untouched");
    }

    #[test]
    fn split_with_active_item_of_an_empty_pane_stays_empty() {
        let mut ws = Workspace::new();
        let right = ws
            .split_with_active_item(0, SplitDirection::Right)
            .expect("split");
        assert!(ws.tabs(right).is_empty());
        assert_eq!(ws.active_pane(), right);
    }

    #[test]
    fn split_with_active_item_of_an_unknown_pane_is_refused() {
        let mut ws = Workspace::new();
        assert_eq!(ws.split_with_active_item(9, SplitDirection::Right), None);
        assert_eq!(ws.pane_ids(), [0]);
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
        assert_eq!(
            root.children[1],
            PaneGroup::Pane(below),
            "nested axis collapsed"
        );
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
}

#[cfg(test)]
pub(crate) mod test_util {
    use crate::app::App;
    use git2::{IndexAddOption, Oid, Repository, Signature};
    use gpui::{
        point, px, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
        TestAppContext, VisualTestContext, WindowHandle,
    };
    use std::fs;

    /// Dispatch a platform-faithful double-click at `position`: a count-1
    /// down/up pair followed by a count-2 down/up pair. The public
    /// `VisualTestContext::simulate_click` helper hardcodes `click_count: 1`,
    /// so double-clicks are dispatched event by event instead.
    pub(crate) fn simulate_double_click(visual: &mut VisualTestContext, position: Point<Pixels>) {
        for click_count in [1, 2] {
            visual.simulate_event(MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count,
                first_mouse: false,
            });
            visual.simulate_event(MouseUpEvent {
                position,
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count,
            });
        }
    }

    /// Drag from `start` to `end` with the left button: down, an activation
    /// nudge past gpui's 2px threshold, the real move, then up.
    pub(crate) fn simulate_drag(
        visual: &mut VisualTestContext,
        start: Point<Pixels>,
        end: Point<Pixels>,
    ) {
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_move(
            start + point(px(4.), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    }

    pub(crate) fn add_app_window(cx: &mut TestAppContext) -> WindowHandle<App> {
        cx.update(gpui_component::init);
        cx.add_window(App::new)
    }

    pub(crate) fn commit_all(repo: &Repository, message: &str, parent_shas: &[String]) -> String {
        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let sig =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
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

    /// Two-commit repo whose head commit changes `alpha.txt` and `nested/beta.txt`.
    pub(crate) fn init_repo_with_two_changed_files() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        fs::create_dir_all(dir.path().join("nested")).expect("create nested dir");
        fs::write(dir.path().join("alpha.txt"), "alpha v1\n").expect("write alpha");
        fs::write(dir.path().join("nested/beta.txt"), "beta v1\n").expect("write beta");
        let first = commit_all(&repo, "Add files", &[]);
        fs::write(dir.path().join("alpha.txt"), "alpha v2\n").expect("update alpha");
        fs::write(dir.path().join("nested/beta.txt"), "beta v2\n").expect("update beta");
        let head = commit_all(&repo, "Update files", std::slice::from_ref(&first));
        drop(repo);
        (dir, head)
    }

    /// Open the fixture repo, select the head commit, and click into the
    /// changeset review screen.
    pub(crate) fn open_changeset(
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

    /// Click the tree row for the given file index, tolerating the
    /// highlight-driven selector rename after a prior click.
    ///
    /// The file tree renders folder rows before file rows, so with the
    /// `nested/` fixture directory the tree rows are: 0 = `nested` folder,
    /// 1 = `nested/beta.txt`, 2 = `alpha.txt`. File index 0 maps to
    /// `alpha.txt` (tree row 2) and file index 1 to `nested/beta.txt`
    /// (tree row 1).
    pub(crate) fn click_file_row(visual: &mut VisualTestContext, index: usize) {
        let bounds = match index {
            0 => visual
                .debug_bounds("changed-file-row-2")
                .or_else(|| visual.debug_bounds("selected-changed-file-row-2")),
            1 => visual
                .debug_bounds("changed-file-row-1")
                .or_else(|| visual.debug_bounds("selected-changed-file-row-1")),
            _ => panic!("unsupported row index"),
        }
        .expect("file row debug bounds");
        visual.simulate_click(bounds.center(), Modifiers::none());
    }

    /// Bounds of the `alpha.txt` tree row (tree row 2; see `click_file_row`).
    pub(crate) fn alpha_row_bounds(visual: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
        visual
            .debug_bounds("changed-file-row-2")
            .expect("file row debug bounds")
    }
}
