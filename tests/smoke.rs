//! Smoke test for the application boot path.
//!
//! Asserts that booting the root view through the gpui test harness yields the
//! placeholder state. Grows with each feature slice that adds to the golden path
//! of `docs/specs/review/workflow.md`: open repo → graph → changeset → click a
//! file → diff renders in a preview tab → double-click pins the tab.

pub mod common;

use common::QueuedPathPicker;
use gpui::{
    AppContext, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext,
};
use gpui_component::Root;
use greviewer::app::{
    bind_app_keys, App, Mode, PathPickerOutcome, ReviewScreen, Selection, OPEN_REPOSITORY_KEYSTROKE,
};
use greviewer::repo::ChangeKind;

/// Dispatch a platform-faithful double-click at `position`: a count-1
/// down/up pair followed by a count-2 down/up pair. The public
/// `simulate_click` helper hardcodes `click_count: 1`.
fn simulate_double_click(visual: &mut VisualTestContext, position: Point<Pixels>) {
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

#[gpui::test]
async fn boots_to_the_placeholder(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
    });

    let picker = QueuedPathPicker::new([]);
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));

    // The contract: the boot path constructs the App entity in the placeholder
    // (no-repository) state without reading or mutating the real recent-repos store.
    window
        .read_with(cx, |app, _cx| {
            assert!(
                matches!(app.mode, Mode::NoRepo),
                "expected the boot path to land on the placeholder",
            );
        })
        .expect("read booted window");
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
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: repo.commits[0].sha.clone()
                    },
                    "the checked-out tip is selected as soon as the repository opens",
                );
                assert_eq!(app.review_screen, ReviewScreen::Graph);
            }
            Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
        })
        .expect("read window");

    let mut visual = VisualTestContext::from_window(*window, cx);
    // The tip is the checked-out commit, selected by default on open; clicking
    // it keeps the selection. Row 0 is always the pending row, so the tip
    // sits at row 1.
    let row_bounds = visual
        .debug_bounds("selected-commit-row-1")
        .expect("commit row debug bounds");
    visual.simulate_click(row_bounds.center(), Modifiers::none());

    let open_bounds = visual
        .debug_bounds("open-changeset")
        .expect("open changeset debug bounds");
    visual.simulate_click(open_bounds.center(), Modifiers::none());

    let changed_file_bounds = visual
        .debug_bounds("changed-file-row-0")
        .expect("changed file row debug bounds");
    visual.simulate_click(changed_file_bounds.center(), Modifiers::none());

    visual
        .debug_bounds("selected-changed-file-row-0")
        .expect("selected changed file row debug bounds");
    visual
        .debug_bounds("file-detail-shell")
        .expect("file detail shell debug bounds");
    visual
        .debug_bounds("file-diff-side-old")
        .expect("old file diff side debug bounds");
    visual
        .debug_bounds("file-diff-side-new")
        .expect("new file diff side debug bounds");
    visual
        .debug_bounds("file-diff-row-removed")
        .expect("removed line row debug bounds");
    visual
        .debug_bounds("file-diff-row-added")
        .expect("added line row debug bounds");
    visual
        .debug_bounds("workspace-tab-0-0")
        .expect("opening a file shows its tab in the tab bar");

    window
        .read_with(cx, |app, _cx| match &app.review_screen {
            ReviewScreen::Changeset { changeset, .. } => {
                assert_eq!(changeset.files.len(), 1);
                assert_eq!(changeset.files[0].path, "hello.txt");
                assert_eq!(changeset.files[0].kind, ChangeKind::Modified);
                assert_eq!(
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("hello.txt".to_string()),
                );
                assert!(
                    app.workspace.is_preview(0, 0),
                    "single-click opens a preview tab"
                );
            }
            ReviewScreen::Graph => panic!("expected changeset review screen"),
        })
        .expect("read changeset state");

    let row_bounds = visual
        .debug_bounds("selected-changed-file-row-0")
        .expect("selected changed file row debug bounds");
    simulate_double_click(&mut visual, row_bounds.center());

    window
        .read_with(cx, |app, _cx| {
            assert_eq!(app.workspace.tabs(0).len(), 1);
            assert!(!app.workspace.is_preview(0, 0), "double-click pins the tab");
        })
        .expect("read pinned state");

    // Split once: a new empty pane opens to the right and becomes active, and
    // the next tree click opens the file there.
    let split_bounds = visual
        .debug_bounds("workspace-split-right-0")
        .expect("split-right control debug bounds");
    visual.simulate_click(split_bounds.center(), Modifiers::none());
    cx.run_until_parked();

    let row_bounds = visual
        .debug_bounds("selected-changed-file-row-0")
        .expect("selected changed file row debug bounds");
    visual.simulate_click(row_bounds.center(), Modifiers::none());
    cx.run_until_parked();

    visual
        .debug_bounds("workspace-tab-1-0")
        .expect("file opens in the new pane's tab bar");
    window
        .read_with(cx, |app, _cx| {
            assert_eq!(app.workspace.pane_ids(), [0, 1]);
            assert_eq!(app.workspace.active_pane(), 1);
            assert_eq!(
                app.workspace
                    .active_item(1)
                    .map(|item| item.path().to_string()),
                Some("hello.txt".to_string()),
            );
        })
        .expect("read split state");
}

/// Booting through the production window shell — the `App` view wrapped in a
/// `gpui_component::Root` — must render the graph sidebar's filter `Input`
/// without panicking. gpui-component's `Input` reaches for `Root` via
/// `Root::read`/`Root::update` during paint (see `gpui_component::input`), so a
/// window whose root entity is a bare `App` crashes at runtime the moment the
/// sidebar's filter field is on screen. This guards `run()`'s Root wrapper: the
/// window root is a `Root`, and the filter field lays out inside that shell.
#[gpui::test]
async fn boots_with_root_shell_renders_filter_input(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        bind_app_keys(cx);
    });

    let dir = common::load_fixture("two-commits");
    let picker = QueuedPathPicker::new([PathPickerOutcome::Picked(dir.path().to_path_buf())]);
    // Mirror `greviewer::run()`: the window's root entity is a `Root` wrapping
    // the `App` view. If this wrapper is dropped, the sidebar's `Input` panics
    // in the real windowing runtime.
    let window = cx.add_window(|window, cx| {
        let app = cx.new(|cx| App::new_with_picker(window, cx, Box::new(picker)));
        Root::new(app, window, cx)
    });

    cx.simulate_keystrokes(*window, OPEN_REPOSITORY_KEYSTROKE);
    cx.run_until_parked();

    // The window root is a `gpui_component::Root` (this closure only type-checks
    // and reads if the root entity is a `Root`), satisfying the Input's
    // `Root::read`/`Root::update` contract.
    window
        .read_with(cx, |_root: &Root, _cx| {})
        .expect("window root is a gpui_component::Root");

    // The graph sidebar's always-visible filter field renders inside the shell.
    let mut visual = VisualTestContext::from_window(*window, cx);
    visual
        .debug_bounds("branch-filter-input")
        .expect("branch filter input renders inside the Root shell in graph mode");
}
