//! Tests for app-shell wiring that must be observable outside the root view.

pub mod common;

use gpui::{Action, Keystroke, TestAppContext};
use std::{cell::RefCell, rc::Rc};

use common::QueuedPathPicker;
use greviewer::app::{
    bind_app_keys, build_app_menus, App, Mode, OpenFailed, OpenRepository, PathPickerOutcome,
    GREVIEWER_MENU_LABEL, OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
};

#[gpui::test]
async fn keymap_binds_cmd_o_to_open_repository(cx: &mut TestAppContext) {
    cx.update(bind_app_keys);

    cx.update(|cx| {
        let action = OpenRepository;
        let typed = [Keystroke::parse(OPEN_REPOSITORY_KEYSTROKE).expect("parse keystroke")];
        let bindings = cx.key_bindings();
        let keymap = bindings.borrow();

        let found = keymap
            .bindings_for_action(&action)
            .any(|binding| binding.match_keystrokes(&typed) == Some(false));

        assert!(
            found,
            "expected {OPEN_REPOSITORY_KEYSTROKE} to bind OpenRepository"
        );
    });
}

#[gpui::test]
async fn cmd_o_dispatch_invokes_repository_picker(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(bind_app_keys);

    let picker = QueuedPathPicker::new([PathPickerOutcome::Cancelled]);
    let observed_picker = picker.clone();
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));

    cx.simulate_keystrokes(*window, OPEN_REPOSITORY_KEYSTROKE);

    assert_eq!(
        observed_picker.requests().len(),
        1,
        "cmd-o should invoke the repository picker"
    );
}

#[gpui::test]
async fn dispatch_action_invokes_repository_picker(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);

    let picker = QueuedPathPicker::new([PathPickerOutcome::Cancelled]);
    let observed_picker = picker.clone();
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));

    cx.dispatch_action(*window, OpenRepository);

    assert_eq!(
        observed_picker.requests().len(),
        1,
        "dispatch_action should invoke the repository picker"
    );
}

#[test]
fn menu_snapshot_contains_open_repository_action() {
    let (_menus, snapshot) = build_app_menus();

    assert!(
        snapshot.contains_action(
            GREVIEWER_MENU_LABEL,
            OPEN_REPOSITORY_MENU_LABEL,
            OpenRepository.name(),
        ),
        "expected Greviewer menu to contain Open Repository action"
    );
}

#[gpui::test]
async fn prompt_uses_repository_picker_options_and_cancel_is_silent(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);

    let picker = QueuedPathPicker::new([PathPickerOutcome::Cancelled]);
    let observed_picker = picker.clone();
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));
    let app_entity = window.entity(cx).expect("get app entity");

    let captured: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let captured_clone = captured.clone();
    let _subscription = app_entity.update(cx, |_, cx| {
        cx.subscribe(&app_entity, move |_, _, event: &OpenFailed, _| {
            captured_clone.borrow_mut().push(event.0.clone());
        })
    });

    window
        .update(cx, |app, window, cx| {
            app.prompt_and_open_repository(window, cx);
        })
        .expect("prompt repository");
    cx.run_until_parked();

    let requests = observed_picker.requests();
    assert_eq!(requests.len(), 1, "picker called exactly once");
    assert!(!requests[0].files);
    assert!(requests[0].directories);
    assert!(!requests[0].multiple);
    assert!(requests[0].prompt.is_none());
    assert!(
        captured.borrow().is_empty(),
        "cancellation emits no OpenFailed"
    );

    window
        .read_with(cx, |app, _cx| {
            assert!(matches!(app.mode, Mode::NoRepo), "mode remains NoRepo");
        })
        .expect("read app state");
}

#[gpui::test]
async fn prompt_picker_failure_emits_open_failed_and_preserves_mode(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);

    let picker = QueuedPathPicker::new([PathPickerOutcome::Failed(
        "Couldn't open the picker: denied.".to_string(),
    )]);
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));
    let app_entity = window.entity(cx).expect("get app entity");

    let captured: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let captured_clone = captured.clone();
    let _subscription = app_entity.update(cx, |_, cx| {
        cx.subscribe(&app_entity, move |_, _, event: &OpenFailed, _| {
            captured_clone.borrow_mut().push(event.0.clone());
        })
    });

    window
        .update(cx, |app, window, cx| {
            app.prompt_and_open_repository(window, cx);
        })
        .expect("prompt repository");
    cx.run_until_parked();

    window
        .read_with(cx, |app, _cx| {
            assert!(matches!(app.mode, Mode::NoRepo), "mode remains NoRepo");
        })
        .expect("read app state");

    let events = captured.borrow();
    assert_eq!(events.len(), 1, "picker failure emits one OpenFailed");
    assert_eq!(events[0], "Couldn't open the picker: denied.");
}

#[gpui::test]
async fn cmd_o_with_valid_repo_opens_repository(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(bind_app_keys);

    let repo_dir = common::load_fixture("two-commits");
    let picker = QueuedPathPicker::new([PathPickerOutcome::Picked(repo_dir.path().to_path_buf())]);
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));

    cx.simulate_keystrokes(*window, OPEN_REPOSITORY_KEYSTROKE);

    window
        .read_with(cx, |app, _cx| match &app.mode {
            Mode::RepoOpen { repo } => {
                let head = repo.head.as_ref().expect("head present");
                assert_eq!(head.summary, "Update hello.txt");
            }
            Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
        })
        .expect("read app state");
}

#[gpui::test]
async fn cmd_o_with_non_repo_emits_open_failed_and_preserves_mode(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(bind_app_keys);

    let non_repo = tempfile::tempdir().expect("create non-repo dir");
    let picker = QueuedPathPicker::new([PathPickerOutcome::Picked(non_repo.path().to_path_buf())]);
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));
    let app_entity = window.entity(cx).expect("get app entity");

    let captured: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let captured_clone = captured.clone();
    let _subscription = app_entity.update(cx, |_, cx| {
        cx.subscribe(&app_entity, move |_, _, event: &OpenFailed, _| {
            captured_clone.borrow_mut().push(event.0.clone());
        })
    });

    cx.simulate_keystrokes(*window, OPEN_REPOSITORY_KEYSTROKE);

    window
        .read_with(cx, |app, _cx| {
            assert!(matches!(app.mode, Mode::NoRepo), "mode remains NoRepo");
        })
        .expect("read app state");

    let events = captured.borrow();
    assert_eq!(events.len(), 1, "non-repo emits one OpenFailed");
    assert!(
        events[0].contains("isn't a Git repository"),
        "expected repository error, got {:?}",
        events[0]
    );
}

#[gpui::test]
async fn cmd_o_cancellation_preserves_mode_and_emits_no_open_failed(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(bind_app_keys);

    let picker = QueuedPathPicker::new([PathPickerOutcome::Cancelled]);
    let window = cx.add_window(|window, cx| App::new_with_picker(window, cx, Box::new(picker)));
    let app_entity = window.entity(cx).expect("get app entity");

    let captured: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let captured_clone = captured.clone();
    let _subscription = app_entity.update(cx, |_, cx| {
        cx.subscribe(&app_entity, move |_, _, event: &OpenFailed, _| {
            captured_clone.borrow_mut().push(event.0.clone());
        })
    });

    cx.simulate_keystrokes(*window, OPEN_REPOSITORY_KEYSTROKE);

    window
        .read_with(cx, |app, _cx| {
            assert!(matches!(app.mode, Mode::NoRepo), "mode remains NoRepo");
        })
        .expect("read app state");
    assert!(captured.borrow().is_empty(), "cancel emits no OpenFailed");
}
