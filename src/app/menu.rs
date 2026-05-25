//! App-shell menu and key binding registration.

use gpui::{Action, App as GpuiApp, KeyBinding, Menu, MenuItem, SharedString};

use super::OpenRepository;

pub const GREVIEWER_MENU_LABEL: &str = "Greviewer";
pub const OPEN_REPOSITORY_MENU_LABEL: &str = "Open Repository\u{2026}";
pub const OPEN_REPOSITORY_KEYSTROKE: &str = "cmd-o";

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

pub fn bind_app_keys(cx: &mut GpuiApp) {
    cx.bind_keys([open_repository_key_binding()]);
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
