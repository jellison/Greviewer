//! Smoke test for the application boot path.
//!
//! Asserts that booting the root view through the gpui test harness yields the
//! placeholder state. Grows with each feature slice that adds to the golden path
//! of `docs/specs/review/workflow.md`.

pub mod common;

use common::QueuedPathPicker;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use greviewer::app::{
    bind_app_keys, App, Mode, PathPickerOutcome, ReviewScreen, Selection, OPEN_REPOSITORY_KEYSTROKE,
};
use greviewer::repo::ChangeKind;

#[gpui::test]
async fn boots_to_the_placeholder(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
    });

    let _window = cx.add_window(App::new);
    // The contract: the boot path constructs the App entity without panicking.
    // Repository-opening behavior is covered by the dispatch smoke test below.
}

#[gpui::test]
async fn boots_open_repo_renders_head_info(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        bind_app_keys(cx);
    });

    let dir = common::load_fixture("two-commits");
    let picker = QueuedPathPicker::new([PathPickerOutcome::Picked(dir.path().to_path_buf())]);
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));

    cx.simulate_keystrokes(*window, OPEN_REPOSITORY_KEYSTROKE);
    cx.run_until_parked();

    window
        .read_with(cx, |app, _cx| match &app.mode {
            Mode::RepoOpen { repo } => {
                let head = repo.head.as_ref().expect("head present");
                assert_eq!(head.summary, "Update hello.txt");
                assert_eq!(repo.commits.len(), 2);
                assert_eq!(repo.commits[0].summary, "Update hello.txt");
                assert_eq!(repo.commits[1].summary, "Add hello.txt");
                assert!(repo.commits[0].is_head);
                assert_eq!(app.selection, Selection::None);
                assert_eq!(app.review_screen, ReviewScreen::Graph);
            }
            Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
        })
        .expect("read window");

    let mut visual = VisualTestContext::from_window(*window, cx);
    let row_bounds = visual
        .debug_bounds("commit-row-0")
        .expect("commit row debug bounds");
    visual.simulate_click(row_bounds.center(), Modifiers::none());

    let open_bounds = visual
        .debug_bounds("open-changeset")
        .expect("open changeset debug bounds");
    visual.simulate_click(open_bounds.center(), Modifiers::none());

    visual
        .debug_bounds("changed-file-row-0")
        .expect("changed file row debug bounds");

    window
        .read_with(cx, |app, _cx| match &app.review_screen {
            ReviewScreen::Changeset { changeset, .. } => {
                assert_eq!(changeset.files.len(), 1);
                assert_eq!(changeset.files[0].path, "hello.txt");
                assert_eq!(changeset.files[0].kind, ChangeKind::Modified);
            }
            ReviewScreen::Graph => panic!("expected changeset review screen"),
        })
        .expect("read changeset state");
}
