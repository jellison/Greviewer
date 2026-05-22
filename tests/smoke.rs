//! Smoke test for the application boot path.
//!
//! Asserts that booting the root view through the gpui test harness yields the
//! placeholder state. Grows with each feature slice that adds to the golden path
//! of `docs/specs/review/workflow.md`.

mod common;

use gpui::TestAppContext;
use greviewer::app::{App, Mode};

#[gpui::test]
async fn boots_to_the_placeholder(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
    });

    let _window = cx.add_window(App::new);
    // The contract: the boot path constructs the App entity without panicking.
    // When the open-repository affordance lands, this test grows to dispatch
    // the open action and assert the graph-mode transition.
}

#[gpui::test]
async fn boots_open_repo_renders_head_info(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
    });

    let dir = common::load_fixture("two-commits");
    let path = dir.path().to_path_buf();

    let window = cx.add_window(App::new);

    window
        .update(cx, |app, window, cx| {
            app.open_repository_at(path, window, cx);
        })
        .expect("update window");

    window
        .read_with(cx, |app, _cx| match &app.mode {
            Mode::RepoOpen { repo } => {
                let head = repo.head.as_ref().expect("head present");
                assert_eq!(head.summary, "Update hello.txt");
            }
            Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
        })
        .expect("read window");
}
