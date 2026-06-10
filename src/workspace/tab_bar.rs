//! Zed-styled tab bar for one workspace pane.
//!
//! One compact strip above the diff: hairline-separated tabs, active tab on
//! the editor background with a top accent line, preview titles in italics,
//! hover-revealed close buttons, split controls in the right corner. The bar
//! of an inactive pane renders dimmed. Behavior contract lives in
//! `docs/specs/review/workflow.md`.

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, rgb, AnyElement, AppContext, ClickEvent, Context, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled,
};
use gpui_component::Icon;

use super::{PaneId, SplitDirection, Workspace};
use crate::app::{change_kind_text, App, FILE_TREE_FONT_FAMILY};
use crate::icons::LucideIcon;
use crate::repo;

const TAB_BAR_HEIGHT: f32 = 32.;
const TAB_TEXT_SIZE: f32 = 13.;
const TAB_DIR_HINT_TEXT_SIZE: f32 = 10.;
const TAB_BAR_BG: u32 = 0x111111;
const TAB_ACTIVE_BG: u32 = 0x171717;
const TAB_BORDER: u32 = 0x2a2a2a;
const TAB_ACCENT: u32 = 0x7da4ff;
const TAB_MUTED_TEXT: u32 = 0x8a8a8a;
const TAB_DEFAULT_TEXT: u32 = 0xe6eef0;
const TAB_CLOSE_HIT_SIZE: f32 = 16.;
const TAB_CLOSE_ICON_SIZE: f32 = 12.;
const TAB_INACTIVE_OPACITY: f32 = 0.75;
const TAB_BAR_INACTIVE_PANE_OPACITY: f32 = 0.6;
const SPLIT_CONTROL_SIZE: f32 = 24.;
const SPLIT_CONTROL_ICON_SIZE: f32 = 14.;

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
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
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

/// The innermost directory name, shown to disambiguate duplicate file names.
fn parent_directory_hint(path: &str) -> Option<String> {
    let (dir, _file) = path.rsplit_once('/')?;
    Some(dir.rsplit('/').next().unwrap_or(dir).to_string())
}

pub fn render_tab_bar(
    workspace: &Workspace,
    pane: PaneId,
    changeset: &repo::ChangeSet,
    scroll: &ScrollHandle,
    cx: &mut Context<App>,
) -> AnyElement {
    // The tab row exists only while the pane holds tabs (Zed's model): an
    // empty pane shows just the placeholder, and dropping a tab anywhere in
    // it moves the tab there (handled by the pane content's drop target).
    // The zero-sized marker lets view tests positively assert the empty
    // branch rendered: gpui's `debug_bounds` map is never cleared between
    // frames, so asserting the absence of a previously painted tab selector
    // is impossible.
    if workspace.tabs(pane).is_empty() {
        return div()
            .debug_selector(move || format!("workspace-tab-bar-empty-{pane}"))
            .into_any_element();
    }

    let pane_active = workspace.active_pane() == pane;

    let mut strip = div()
        .id(SharedString::from(format!("workspace-tab-strip-{pane}")))
        .flex()
        .items_center()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_x_scroll()
        .track_scroll(scroll)
        .drag_over::<DraggedTab>(|style, _drag, _window, _cx| style.bg(rgb(0x1d2733)))
        .on_drop(cx.listener(move |app, drag: &DraggedTab, _window, cx| {
            let end = app.workspace.tabs(pane).len();
            app.move_workspace_tab(drag.pane, drag.index, pane, end, cx);
        }));

    for (index, item) in workspace.tabs(pane).iter().enumerate() {
        let active = workspace.active_index(pane) == Some(index);
        let preview = workspace.is_preview(pane, index);
        let title = item.tab_title().to_string();
        let duplicate_title = workspace
            .tabs(pane)
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
        let tab_selector = format!("workspace-tab-{pane}-{index}");
        let close_selector = format!("workspace-tab-close-{pane}-{index}");
        let group_name = format!("workspace-tab-{pane}-{index}");

        let close_button = div()
            .id(SharedString::from(close_selector.clone()))
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
                app.close_workspace_tab(pane, index, cx);
            }))
            .child(
                Icon::new(LucideIcon::X)
                    .size(px(TAB_CLOSE_ICON_SIZE))
                    .text_color(rgb(TAB_MUTED_TEXT)),
            );

        let tab = div()
            .id(SharedString::from(tab_selector.clone()))
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
                    app.promote_workspace_tab(pane, index, cx);
                } else {
                    app.activate_workspace_tab(pane, index, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |app, _event: &MouseDownEvent, _window, cx| {
                    app.close_workspace_tab(pane, index, cx);
                }),
            )
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
            .on_drop(cx.listener(move |app, drag: &DraggedTab, _window, cx| {
                cx.stop_propagation();
                app.move_workspace_tab(drag.pane, drag.index, pane, index, cx);
            }))
            .child(
                div()
                    .text_size(px(TAB_TEXT_SIZE))
                    .font_family(FILE_TREE_FONT_FAMILY)
                    .text_color(title_color)
                    .whitespace_nowrap()
                    .when(preview, |label| label.italic())
                    .child(title.clone()),
            )
            .when_some(parent_hint, |tab, hint| {
                tab.child(
                    div()
                        .text_size(px(TAB_DIR_HINT_TEXT_SIZE))
                        .font_family(FILE_TREE_FONT_FAMILY)
                        .text_color(rgb(TAB_MUTED_TEXT))
                        .whitespace_nowrap()
                        .child(hint),
                )
            })
            .child(close_button);

        strip = strip.child(tab);
    }

    div()
        .id(SharedString::from(format!("workspace-tab-bar-{pane}")))
        .debug_selector(move || format!("workspace-tab-bar-{pane}"))
        .flex()
        .items_center()
        .w_full()
        .h(px(TAB_BAR_HEIGHT))
        .flex_none()
        .bg(rgb(TAB_BAR_BG))
        .border_b_1()
        .border_color(rgb(TAB_BORDER))
        .when(!pane_active, |bar| {
            bar.opacity(TAB_BAR_INACTIVE_PANE_OPACITY)
        })
        .child(strip)
        .child(
            div()
                .flex()
                .items_center()
                .flex_none()
                .gap_1()
                .px_2()
                .h_full()
                .child(split_control(
                    pane,
                    LucideIcon::Columns2,
                    format!("workspace-split-right-{pane}"),
                    SplitDirection::Right,
                    cx,
                ))
                .child(split_control(
                    pane,
                    LucideIcon::Rows2,
                    format!("workspace-split-down-{pane}"),
                    SplitDirection::Down,
                    cx,
                )),
        )
        .into_any_element()
}

/// One corner control that splits `pane` in `direction` when clicked.
fn split_control(
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
        .w(px(SPLIT_CONTROL_SIZE))
        .h(px(SPLIT_CONTROL_SIZE))
        .rounded(px(2.))
        .cursor_pointer()
        .hover(|button| button.bg(rgb(TAB_BORDER)))
        .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
            app.split_workspace_pane(pane, direction, cx);
        }))
        .child(
            Icon::new(icon)
                .size(px(SPLIT_CONTROL_ICON_SIZE))
                .text_color(rgb(TAB_MUTED_TEXT)),
        )
}

#[cfg(test)]
mod tests {
    use crate::workspace::test_util::{
        alpha_row_bounds, click_file_row, open_changeset, simulate_double_click,
    };
    use gpui::{Modifiers, MouseButton, TestAppContext};

    #[gpui::test]
    async fn single_clicks_share_one_preview_tab(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        click_file_row(&mut visual, 0);
        visual
            .debug_bounds("workspace-tab-0-0")
            .expect("first tab renders");
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("workspace-tab-0-1").is_none(),
            "second single-click must reuse the preview tab, not add a tab"
        );
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1);
                assert!(app.workspace.is_preview(0, 0));
                assert_eq!(
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("nested/beta.txt".to_string()),
                );
            })
            .expect("read workspace state");
    }

    #[gpui::test]
    async fn double_clicking_a_file_row_pins_the_tab(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let row_bounds = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row_bounds.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.workspace.tabs(0).len(),
                    2,
                    "pinned tab plus new preview"
                );
                assert!(
                    !app.workspace.is_preview(0, 0),
                    "double-clicked tab is pinned"
                );
                assert!(app.workspace.is_preview(0, 1));
            })
            .expect("read workspace state");
    }

    #[gpui::test]
    async fn clicking_a_tab_activates_it_and_double_click_promotes(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let row_bounds = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row_bounds.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Click the first (pinned) tab: it activates; tree highlight stays put.
        let tab0 = visual.debug_bounds("workspace-tab-0-0").expect("tab 0");
        visual.simulate_click(tab0.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.active_index(0), Some(0));
                assert_eq!(
                    app.file_tree_highlight_path,
                    Some("nested/beta.txt".to_string()),
                    "tab activation must not move the tree highlight"
                );
            })
            .expect("read activation state");

        // Double-click the preview tab: it pins in place.
        let tab1 = visual.debug_bounds("workspace-tab-0-1").expect("tab 1");
        simulate_double_click(&mut visual, tab1.center());
        window
            .read_with(cx, |app, _cx| {
                assert!(
                    !app.workspace.is_preview(0, 1),
                    "double-clicked tab is promoted"
                );
            })
            .expect("read promotion state");
    }

    #[gpui::test]
    async fn close_button_and_middle_click_close_tabs(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_changeset(cx);

        let row_bounds = alpha_row_bounds(&mut visual);
        simulate_double_click(&mut visual, row_bounds.center());
        click_file_row(&mut visual, 1);
        cx.run_until_parked();

        // Close the active (preview) tab via its close button.
        let close1 = visual
            .debug_bounds("workspace-tab-close-0-1")
            .expect("close button on active tab");
        visual.simulate_click(close1.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs(0).len(), 1);
                assert_eq!(
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("alpha.txt".to_string()),
                    "left neighbor becomes active"
                );
            })
            .expect("read state after close-button close");

        // Middle-click closes the remaining tab; the placeholder returns.
        let tab0 = visual.debug_bounds("workspace-tab-0-0").expect("tab 0");
        visual.simulate_mouse_down(tab0.center(), MouseButton::Middle, Modifiers::none());
        visual.simulate_mouse_up(tab0.center(), MouseButton::Middle, Modifiers::none());
        cx.run_until_parked();

        // gpui never clears `debug_bounds` entries between frames, so the
        // closed tab's selector lingers in the map; the empty-bar marker is
        // the positive signal that the latest frame rendered no tabs.
        visual
            .debug_bounds("workspace-tab-bar-empty-0")
            .expect("tab bar renders its empty branch");
        visual
            .debug_bounds("file-detail-empty")
            .expect("placeholder returns when every tab is closed");
        window
            .read_with(cx, |app, _cx| assert!(app.workspace.tabs(0).is_empty()))
            .expect("read emptied workspace");
    }
}
