//! App-shell menu and key binding registration.

use gpui::{Action, App as GpuiApp, KeyBinding, Menu, MenuItem, SharedString};

use super::{
    ActivateNextTab, ActivatePreviousTab, CloseActivePane, CloseActiveTab, DiffAddComment,
    DiffCancelSelection, DiffCopy, DiffMoveDocEnd, DiffMoveDocStart, DiffMoveDown, DiffMoveLeft,
    DiffMoveLineEnd, DiffMoveLineStart, DiffMoveRight, DiffMoveUp, DiffMoveWordLeft,
    DiffMoveWordRight, DiffSelectAll, DiffSelectDocEnd, DiffSelectDocStart, DiffSelectDown,
    DiffSelectLeft, DiffSelectLineEnd, DiffSelectLineStart, DiffSelectRight, DiffSelectUp,
    DiffSelectWordLeft, DiffSelectWordRight, NextChangeBlock, OpenChangeset, OpenRepository,
    PreviousChangeBlock, QuitApplication, SplitPaneDown, SplitPaneLeft, SplitPaneRight,
    SplitPaneUp,
};

pub const GREVIEWER_MENU_LABEL: &str = "Greviewer";
pub const OPEN_REPOSITORY_MENU_LABEL: &str = "Open Repository\u{2026}";
pub const OPEN_REPOSITORY_KEYSTROKE: &str = "cmd-o";
pub const QUIT_APPLICATION_KEYSTROKE: &str = "cmd-q";
pub const CLOSE_ACTIVE_TAB_KEYSTROKE: &str = "cmd-w";
pub const ACTIVATE_NEXT_TAB_KEYSTROKE: &str = "ctrl-tab";
pub const ACTIVATE_PREVIOUS_TAB_KEYSTROKE: &str = "ctrl-shift-tab";
pub const SPLIT_PANE_LEFT_KEYSTROKE: &str = "cmd-k left";
pub const SPLIT_PANE_RIGHT_KEYSTROKE: &str = "cmd-k right";
pub const SPLIT_PANE_UP_KEYSTROKE: &str = "cmd-k up";
pub const SPLIT_PANE_DOWN_KEYSTROKE: &str = "cmd-k down";
pub const CLOSE_ACTIVE_PANE_KEYSTROKE: &str = "cmd-k w";
/// Graph-screen selection shortcut: enter opens the changeset for the
/// current selection. An app-level binding; the action handler no-ops
/// outside graph mode, and focused components (such as the branch-filter
/// input) bind this key in their own context and win.
pub const OPEN_CHANGESET_KEYSTROKE: &str = "enter";
pub const NEXT_CHANGE_BLOCK_KEYSTROKE: &str = "alt-cmd-down";
pub const PREVIOUS_CHANGE_BLOCK_KEYSTROKE: &str = "alt-cmd-up";

pub const DIFF_PANE_CONTEXT: &str = "DiffPane";
pub const DIFF_MOVE_LEFT_KEYSTROKE: &str = "left";
pub const DIFF_MOVE_RIGHT_KEYSTROKE: &str = "right";
pub const DIFF_MOVE_UP_KEYSTROKE: &str = "up";
pub const DIFF_MOVE_DOWN_KEYSTROKE: &str = "down";
pub const DIFF_MOVE_WORD_LEFT_KEYSTROKE: &str = "alt-left";
pub const DIFF_MOVE_WORD_RIGHT_KEYSTROKE: &str = "alt-right";
pub const DIFF_MOVE_LINE_START_KEYSTROKE: &str = "cmd-left";
pub const DIFF_MOVE_LINE_END_KEYSTROKE: &str = "cmd-right";
pub const DIFF_MOVE_DOC_START_KEYSTROKE: &str = "cmd-up";
pub const DIFF_MOVE_DOC_END_KEYSTROKE: &str = "cmd-down";
pub const DIFF_SELECT_LEFT_KEYSTROKE: &str = "shift-left";
pub const DIFF_SELECT_RIGHT_KEYSTROKE: &str = "shift-right";
pub const DIFF_SELECT_UP_KEYSTROKE: &str = "shift-up";
pub const DIFF_SELECT_DOWN_KEYSTROKE: &str = "shift-down";
pub const DIFF_SELECT_WORD_LEFT_KEYSTROKE: &str = "alt-shift-left";
pub const DIFF_SELECT_WORD_RIGHT_KEYSTROKE: &str = "alt-shift-right";
pub const DIFF_SELECT_LINE_START_KEYSTROKE: &str = "cmd-shift-left";
pub const DIFF_SELECT_LINE_END_KEYSTROKE: &str = "cmd-shift-right";
pub const DIFF_SELECT_DOC_START_KEYSTROKE: &str = "cmd-shift-up";
pub const DIFF_SELECT_DOC_END_KEYSTROKE: &str = "cmd-shift-down";
pub const DIFF_SELECT_ALL_KEYSTROKE: &str = "cmd-a";
pub const DIFF_COPY_KEYSTROKE: &str = "cmd-c";
pub const DIFF_CANCEL_SELECTION_KEYSTROKE: &str = "escape";
pub const DIFF_ADD_COMMENT_KEYSTROKE: &str = "cmd-shift-c";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSnapshot {
    menus: Vec<MenuSnapshotMenu>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSnapshotMenu {
    pub name: String,
    pub items: Vec<MenuSnapshotItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuSnapshotItem {
    Action { name: String, action_name: String },
}

impl MenuSnapshot {
    pub fn contains_action(&self, menu_name: &str, item_name: &str, action_name: &str) -> bool {
        self.menus.iter().any(|menu| {
            menu.name == menu_name
                && menu.items.iter().any(|item| {
                    matches!(
                        item,
                        MenuSnapshotItem::Action { name, action_name: stored_action }
                            if name == item_name && stored_action == action_name
                    )
                })
        })
    }
}

pub fn open_repository_key_binding() -> KeyBinding {
    KeyBinding::new(OPEN_REPOSITORY_KEYSTROKE, OpenRepository, None)
}

pub fn quit_application_key_binding() -> KeyBinding {
    KeyBinding::new(QUIT_APPLICATION_KEYSTROKE, QuitApplication, None)
}

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
        KeyBinding::new(OPEN_CHANGESET_KEYSTROKE, OpenChangeset, None),
        KeyBinding::new(NEXT_CHANGE_BLOCK_KEYSTROKE, NextChangeBlock, None),
        KeyBinding::new(PREVIOUS_CHANGE_BLOCK_KEYSTROKE, PreviousChangeBlock, None),
        KeyBinding::new(
            DIFF_MOVE_LEFT_KEYSTROKE,
            DiffMoveLeft,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_MOVE_RIGHT_KEYSTROKE,
            DiffMoveRight,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(DIFF_MOVE_UP_KEYSTROKE, DiffMoveUp, Some(DIFF_PANE_CONTEXT)),
        KeyBinding::new(
            DIFF_MOVE_DOWN_KEYSTROKE,
            DiffMoveDown,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_MOVE_WORD_LEFT_KEYSTROKE,
            DiffMoveWordLeft,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_MOVE_WORD_RIGHT_KEYSTROKE,
            DiffMoveWordRight,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_MOVE_LINE_START_KEYSTROKE,
            DiffMoveLineStart,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_MOVE_LINE_END_KEYSTROKE,
            DiffMoveLineEnd,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_MOVE_DOC_START_KEYSTROKE,
            DiffMoveDocStart,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_MOVE_DOC_END_KEYSTROKE,
            DiffMoveDocEnd,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_LEFT_KEYSTROKE,
            DiffSelectLeft,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_RIGHT_KEYSTROKE,
            DiffSelectRight,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_UP_KEYSTROKE,
            DiffSelectUp,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_DOWN_KEYSTROKE,
            DiffSelectDown,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_WORD_LEFT_KEYSTROKE,
            DiffSelectWordLeft,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_WORD_RIGHT_KEYSTROKE,
            DiffSelectWordRight,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_LINE_START_KEYSTROKE,
            DiffSelectLineStart,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_LINE_END_KEYSTROKE,
            DiffSelectLineEnd,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_DOC_START_KEYSTROKE,
            DiffSelectDocStart,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_DOC_END_KEYSTROKE,
            DiffSelectDocEnd,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_SELECT_ALL_KEYSTROKE,
            DiffSelectAll,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(DIFF_COPY_KEYSTROKE, DiffCopy, Some(DIFF_PANE_CONTEXT)),
        KeyBinding::new(
            DIFF_CANCEL_SELECTION_KEYSTROKE,
            DiffCancelSelection,
            Some(DIFF_PANE_CONTEXT),
        ),
        KeyBinding::new(
            DIFF_ADD_COMMENT_KEYSTROKE,
            DiffAddComment,
            Some(DIFF_PANE_CONTEXT),
        ),
    ]);
}

pub fn build_app_menus() -> (Vec<Menu>, MenuSnapshot) {
    let menus = vec![Menu {
        name: SharedString::from(GREVIEWER_MENU_LABEL),
        items: vec![MenuItem::action(OPEN_REPOSITORY_MENU_LABEL, OpenRepository)],
    }];

    let snapshot = MenuSnapshot {
        menus: vec![MenuSnapshotMenu {
            name: GREVIEWER_MENU_LABEL.to_string(),
            items: vec![MenuSnapshotItem::Action {
                name: OPEN_REPOSITORY_MENU_LABEL.to_string(),
                action_name: OpenRepository.name().to_string(),
            }],
        }],
    };

    (menus, snapshot)
}
