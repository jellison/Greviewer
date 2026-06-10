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
        self.pane
            .active
            .and_then(|index| self.pane.tabs.get(index))
            .map(|item| item.as_ref())
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
        assert!(
            !ws.open_pinned(item("a.rs")),
            "already active: no content change"
        );
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

    #[test]
    fn closing_the_active_tab_activates_the_right_neighbor() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        ws.activate_tab(1);
        assert!(ws.close_tab(1));
        assert_eq!(paths(&ws), ["a.rs", "c.rs"]);
        assert_eq!(
            ws.active_index(),
            Some(1),
            "right neighbor (c.rs) becomes active"
        );
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
    fn closing_a_tab_left_of_both_preview_and_active_shifts_both_indices() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_pinned(item("b.rs"));
        ws.open_preview(item("c.rs"));
        assert!(
            !ws.close_tab(0),
            "active content unchanged by closing a left tab"
        );
        assert_eq!(paths(&ws), ["b.rs", "c.rs"]);
        assert!(ws.is_preview(1), "preview index shifts with its tab");
        assert_eq!(ws.active_index(), Some(1));
        assert_eq!(ws.active_item().unwrap().path(), "c.rs");
    }

    #[test]
    fn promote_tab_pins_only_the_preview_tab() {
        let mut ws = Workspace::new();
        ws.open_preview(item("a.rs"));
        ws.promote_tab(0);
        assert!(!ws.is_preview(0));
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

    #[test]
    fn closing_a_tab_right_of_active_and_preview_shifts_nothing() {
        let mut ws = Workspace::new();
        ws.open_pinned(item("a.rs"));
        ws.open_preview(item("b.rs"));
        ws.open_pinned(item("c.rs"));
        ws.activate_tab(0);
        assert!(!ws.close_tab(2), "active content unchanged");
        assert_eq!(paths(&ws), ["a.rs", "b.rs"]);
        assert_eq!(ws.active_index(), Some(0));
        assert!(
            ws.is_preview(1),
            "preview left of the closed tab keeps its index"
        );
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, VisualTestContext,
    };

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
}
