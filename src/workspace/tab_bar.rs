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
        // Zero-sized marker so view tests can positively assert the bar
        // rendered its empty branch: gpui's `debug_bounds` map is never
        // cleared between frames, so asserting the absence of a previously
        // painted tab selector is impossible.
        return div()
            .debug_selector(|| "workspace-tab-bar-empty".into())
            .into_any_element();
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

        bar = bar.child(tab);
    }

    bar.into_any_element()
}

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
    fn init_repo_with_two_changed_files() -> (tempfile::TempDir, String) {
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

    /// Click the tree row for the given file index, tolerating the
    /// highlight-driven selector rename after a prior click.
    ///
    /// The file tree renders folder rows before file rows, so with the
    /// `nested/` fixture directory the tree rows are: 0 = `nested` folder,
    /// 1 = `nested/beta.txt`, 2 = `alpha.txt`. File index 0 maps to
    /// `alpha.txt` (tree row 2) and file index 1 to `nested/beta.txt`
    /// (tree row 1).
    fn click_file_row(visual: &mut VisualTestContext, index: usize) {
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
    fn alpha_row_bounds(visual: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
        visual
            .debug_bounds("changed-file-row-2")
            .expect("file row debug bounds")
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
                    app.workspace
                        .active_item()
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
                assert_eq!(app.workspace.tabs().len(), 2, "pinned tab plus new preview");
                assert!(!app.workspace.is_preview(0), "double-clicked tab is pinned");
                assert!(app.workspace.is_preview(1));
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
        simulate_double_click(&mut visual, tab1.center());
        window
            .read_with(cx, |app, _cx| {
                assert!(
                    !app.workspace.is_preview(1),
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
            .debug_bounds("workspace-tab-close-1")
            .expect("close button on active tab");
        visual.simulate_click(close1.center(), Modifiers::none());
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.tabs().len(), 1);
                assert_eq!(
                    app.workspace
                        .active_item()
                        .map(|item| item.path().to_string()),
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

        // gpui never clears `debug_bounds` entries between frames, so the
        // closed tab's selector lingers in the map; the empty-bar marker is
        // the positive signal that the latest frame rendered no tabs.
        visual
            .debug_bounds("workspace-tab-bar-empty")
            .expect("tab bar renders its empty branch");
        visual
            .debug_bounds("file-detail-empty")
            .expect("placeholder returns when every tab is closed");
        window
            .read_with(cx, |app, _cx| assert!(app.workspace.tabs().is_empty()))
            .expect("read emptied workspace");
    }
}
