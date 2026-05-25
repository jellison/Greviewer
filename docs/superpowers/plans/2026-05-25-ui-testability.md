# UI Testability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Greviewer's existing open-repository UI affordance testable through gpui dispatch, then fix the broken `cmd-o`/menu action path under that regression test.

**Architecture:** Keep the app shell in `src/app.rs`, with app-owned submodules under `src/app/` for menu registration and path picking. Replace direct OS picker calls with a `PathPicker` trait so tests can queue outcomes while production still uses `cx.prompt_for_paths`. Move open-repository action handling into the rendered root view's dispatch tree so gpui keystroke/action dispatch reaches it.

**Tech Stack:** Rust 2021, gpui `0.2`, gpui-component `0.5`, git2 `0.20`, `#[gpui::test]`, Cargo integration tests.

---

## Pre-Flight

`bin/check` currently fails in this workspace before compiling Greviewer because gpui's Metal shader build cannot find the Xcode Metal toolchain:

```text
metal shader compilation failed:
error: cannot execute tool 'metal' due to missing Metal Toolchain; use: xcodebuild -downloadComponent MetalToolchain
```

Before executing implementation tasks, install the missing local prerequisite:

```bash
xcodebuild -downloadComponent MetalToolchain
```

After that, every task's targeted test command should run. The final task still requires `bin/check`.

## File Structure

- Create `docs/adr/0004-ui-testability.md`: binding architectural rule for dispatch-level UI tests and accepted gaps.
- Modify `src/app.rs`: root app state, picker injection, root focus handle, root-level `OpenRepository` action listener, existing view tests.
- Create `src/app/menu.rs`: one source of truth for menu construction, key binding construction, and `MenuSnapshot`.
- Create `src/app/path_picker.rs`: `PathPicker` trait, production gpui implementation, picker outcome enum, repository prompt options.
- Modify `src/lib.rs`: use app shell helpers from `src/app/menu.rs`; remove the broken global `cx.on_action` handler.
- Modify `tests/common/mod.rs`: add `QueuedPathPicker` test helper that records prompt options and returns queued picker outcomes.
- Create `tests/app_shell.rs`: integration-style gpui tests for key bindings, menu snapshot, dispatch, and picker outcomes.
- Modify `tests/smoke.rs`: open the fixture repository through `cmd-o` dispatch instead of direct `open_repository_at`.

### Task 1: ADR-0004

**Files:**
- Create: `docs/adr/0004-ui-testability.md`
- Test: `bin/check`

- [ ] **Step 1: Write ADR-0004**

Create `docs/adr/0004-ui-testability.md` with this content:

```markdown
# ADR-0004: UI testability — dispatch-level coverage for user affordances

**Status:** Accepted

**Date:** 2026-05-25

---

## Context and Problem Statement

Greviewer has already shipped a concrete UI wiring defect: the open-repository menu item and `cmd-o` binding compile, pass the test suite, and still do not visibly dispatch in the running app. The failure was possible because tests called application methods directly instead of exercising gpui's action and key-dispatch chain.

In an AI-authored desktop app, that gap is architectural. Method-level tests prove state transitions, but they do not prove that a user can reach those transitions through the rendered UI. Agents need a binding rule that distinguishes focused state tests from affordance tests.

## Decision

Every user-facing UI affordance must have automated coverage that drives gpui's public interaction boundary: keystroke dispatch, action dispatch, typed events, or rendered-view interaction. Calling private helper methods or app-internal methods is allowed for focused unit coverage, but it does not satisfy affordance coverage.

For the current open-repository slice, the required coverage is:

- the live keymap binds `cmd-o` to `OpenRepository`;
- Greviewer registers the `Greviewer -> Open Repository...` menu item for `OpenRepository`;
- dispatching `OpenRepository` through gpui invokes the folder picker;
- picker success, cancellation, picker failure, and invalid repository selections preserve or update app state as specified.

## Implementation Guidance

Use dependency injection for OS or platform services that gpui's test platform cannot drive directly. The first required abstraction is `PathPicker`, because `prompt_for_paths` is unimplemented in gpui's test platform.

Use typed app events as public observability for UI side effects that third-party widgets do not expose. The existing `OpenFailed` event is the pattern for notification verification because `gpui-component` keeps notification fields private.

Use registration snapshots only where gpui does not expose state after registration. The first snapshot is `MenuSnapshot`, built from the same source that produces the gpui menus.

## Accepted Gaps

The automated suite does not claim to verify real native menu clicks, real OS folder dialogs, real display rendering, window-manager behavior, or pixel snapshots. Those remain manual QA gaps until the app has enough visual complexity or failure history to justify a new ADR.

## Consequences

Adding a view, menu item, key binding, command, or other UI affordance without dispatch-level test coverage is a review defect. Direct method tests can remain as lower-level coverage, but they cannot be cited as proof that the UI wiring works.

---

## References

- ADR-0003: Testing strategy
- `docs/superpowers/specs/2026-05-25-ui-testability-design.md`
```

- [ ] **Step 2: Run the documentation verification command**

Run:

```bash
bin/check
```

Expected: PASS after the Metal toolchain prerequisite is installed.

- [ ] **Step 3: Commit**

```bash
git add docs/adr/0004-ui-testability.md
git commit -m "docs(adr): require dispatch-level UI tests"
```

### Task 2: Menu Snapshot And Key Binding Helpers

**Files:**
- Create: `src/app/menu.rs`
- Modify: `src/app.rs`
- Modify: `src/lib.rs`
- Create: `tests/app_shell.rs`

- [ ] **Step 1: Write failing tests for app shell registration**

Create `tests/app_shell.rs`:

```rust
//! Tests for app-shell wiring that must be observable outside the root view.

mod common;

use gpui::{Action, AppContext as _, Keystroke, TestAppContext};
use greviewer::app::{
    bind_app_keys, build_app_menus, OpenRepository, GREVIEWER_MENU_LABEL,
    OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
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

        assert!(found, "expected {OPEN_REPOSITORY_KEYSTROKE} to bind OpenRepository");
    });
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --test app_shell
```

Expected: FAIL to compile because `bind_app_keys`, `build_app_menus`, `GREVIEWER_MENU_LABEL`, `OPEN_REPOSITORY_KEYSTROKE`, and `OPEN_REPOSITORY_MENU_LABEL` do not exist yet.

- [ ] **Step 3: Implement menu and key binding helpers**

Create `src/app/menu.rs`:

```rust
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
        items: vec![MenuItem::action(
            OPEN_REPOSITORY_MENU_LABEL,
            OpenRepository,
        )],
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
```

Modify the top of `src/app.rs` to declare and re-export the module:

```rust
pub mod menu;

pub use menu::{
    bind_app_keys, build_app_menus, MenuSnapshot, GREVIEWER_MENU_LABEL,
    OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
};
```

Modify `src/lib.rs` so production uses the helpers:

```rust
use gpui::{
    px, size, App, AppContext, Application, Bounds, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions,
};

pub fn run() {
    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);

        app::bind_app_keys(cx);

        let (menus, _menu_snapshot) = app::build_app_menus();
        cx.set_menus(menus);

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("Greviewer")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| app::App::new(window, cx)),
        )
        .expect("opening the main window");

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.activate(true);
    });
}
```

Do not keep the old `cx.bind_keys([...])`, `cx.set_menus(...)`, or `cx.on_action(...)` blocks in `src/lib.rs`; the global action handler is the broken path this slice replaces.

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test --test app_shell
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/app/menu.rs src/lib.rs tests/app_shell.rs
git commit -m "test(app): snapshot menu and key binding wiring"
```

### Task 3: PathPicker Abstraction

**Files:**
- Create: `src/app/path_picker.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Write a focused unit test for repository picker options**

Create `src/app/path_picker.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::repository_prompt_options;

    #[test]
    fn repository_prompt_options_selects_one_directory() {
        let options = repository_prompt_options();

        assert!(!options.files, "repository picker should not allow files");
        assert!(options.directories, "repository picker should allow directories");
        assert!(!options.multiple, "repository picker should select one directory");
        assert!(options.prompt.is_none(), "repository picker has no custom prompt yet");
    }
}
```

Modify the top of `src/app.rs` so the module is compiled:

```rust
pub mod menu;
pub mod path_picker;
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test app::path_picker::tests::repository_prompt_options_selects_one_directory
```

Expected: FAIL to compile because `repository_prompt_options` does not exist yet.

- [ ] **Step 3: Implement `PathPicker`**

Create `src/app/path_picker.rs`:

```rust
//! Folder-picker abstraction for app-shell workflows.

use gpui::{Context, PathPromptOptions, Task, Window};
use std::path::PathBuf;

use super::App;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPickerOutcome {
    Picked(PathBuf),
    Cancelled,
    Failed(String),
}

pub trait PathPicker: 'static {
    fn pick_path(
        &self,
        options: PathPromptOptions,
        window: &mut Window,
        cx: &mut Context<App>,
    ) -> Task<PathPickerOutcome>;
}

#[derive(Default)]
pub struct GpuiPathPicker;

impl PathPicker for GpuiPathPicker {
    fn pick_path(
        &self,
        options: PathPromptOptions,
        window: &mut Window,
        cx: &mut Context<App>,
    ) -> Task<PathPickerOutcome> {
        let receiver = cx.prompt_for_paths(options);

        cx.spawn_in(window, async move |_, _| match receiver.await {
            Ok(Ok(Some(mut paths))) if !paths.is_empty() => {
                PathPickerOutcome::Picked(paths.remove(0))
            }
            Ok(Ok(_)) => PathPickerOutcome::Cancelled,
            Ok(Err(err)) => {
                PathPickerOutcome::Failed(format!("Couldn't open the picker: {err}."))
            }
            Err(_recv_err) => PathPickerOutcome::Cancelled,
        })
    }
}

pub fn repository_prompt_options() -> PathPromptOptions {
    PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::repository_prompt_options;

    #[test]
    fn repository_prompt_options_selects_one_directory() {
        let options = repository_prompt_options();

        assert!(!options.files, "repository picker should not allow files");
        assert!(options.directories, "repository picker should allow directories");
        assert!(!options.multiple, "repository picker should select one directory");
        assert!(options.prompt.is_none(), "repository picker has no custom prompt yet");
    }
}
```

Modify the top of `src/app.rs`:

```rust
pub mod menu;
pub mod path_picker;

pub use menu::{
    bind_app_keys, build_app_menus, MenuSnapshot, GREVIEWER_MENU_LABEL,
    OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
};
pub use path_picker::{repository_prompt_options, GpuiPathPicker, PathPicker, PathPickerOutcome};
```

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
cargo test app::path_picker::tests::repository_prompt_options_selects_one_directory
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/app/path_picker.rs
git commit -m "feat(app): add path picker abstraction"
```

### Task 4: Inject PathPicker Into App

**Files:**
- Modify: `src/app.rs`
- Modify: `tests/common/mod.rs`
- Modify: `tests/app_shell.rs`

- [ ] **Step 1: Add a queued picker test helper**

Append this code to `tests/common/mod.rs`:

```rust
use gpui::{Context, PathPromptOptions, Task, Window};
use greviewer::app::{App, PathPicker, PathPickerOutcome};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Clone)]
pub struct QueuedPathPicker {
    state: Rc<RefCell<QueuedPathPickerState>>,
}

struct QueuedPathPickerState {
    requests: Vec<PathPromptOptions>,
    outcomes: VecDeque<PathPickerOutcome>,
}

impl QueuedPathPicker {
    pub fn new(outcomes: impl IntoIterator<Item = PathPickerOutcome>) -> Self {
        Self {
            state: Rc::new(RefCell::new(QueuedPathPickerState {
                requests: Vec::new(),
                outcomes: outcomes.into_iter().collect(),
            })),
        }
    }

    pub fn requests(&self) -> Vec<PathPromptOptions> {
        self.state.borrow().requests.clone()
    }
}

impl PathPicker for QueuedPathPicker {
    fn pick_path(
        &self,
        options: PathPromptOptions,
        window: &mut Window,
        cx: &mut Context<App>,
    ) -> Task<PathPickerOutcome> {
        let outcome = {
            let mut state = self.state.borrow_mut();
            state.requests.push(options);
            state
                .outcomes
                .pop_front()
                .expect("queued path picker outcome")
        };

        cx.spawn_in(window, async move |_, _| outcome)
    }
}
```

If this introduces duplicate imports in `tests/common/mod.rs`, merge them into the existing `use` blocks instead of adding a second import of the same `std` item.

- [ ] **Step 2: Write failing tests for direct picker flow**

Append these tests to `tests/app_shell.rs`:

```rust
use std::{cell::RefCell, rc::Rc};

use common::QueuedPathPicker;
use greviewer::app::{App, Mode, OpenFailed, PathPickerOutcome};

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
    assert!(captured.borrow().is_empty(), "cancellation emits no OpenFailed");

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
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test --test app_shell prompt_
```

Expected: FAIL to compile because `App::new_with_picker` does not exist and `App` does not store a picker yet.

- [ ] **Step 4: Inject the picker and refactor prompt handling**

Modify `src/app.rs` imports:

```rust
use gpui::{
    actions, div, px, rgb, AppContext, Context, Entity, EventEmitter, FocusHandle, IntoElement,
    ParentElement, Render, Styled, Window,
};
```

Modify `App`:

```rust
pub struct App {
    pub mode: Mode,
    notifications: Entity<NotificationList>,
    path_picker: Box<dyn PathPicker>,
    focus_handle: FocusHandle,
}
```

Replace `App::new` with:

```rust
impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_with_picker(window, cx, Box::new(GpuiPathPicker))
    }

    pub fn new_with_picker(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
    ) -> Self {
        let notifications = cx.new(|cx| NotificationList::new(window, cx));
        let focus_handle = cx.focus_handle();

        cx.on_next_frame(window, |app, window, _cx| {
            window.focus(&app.focus_handle);
        });

        Self {
            mode: Mode::NoRepo,
            notifications,
            path_picker,
            focus_handle,
        }
    }
}
```

Add these helper methods inside `impl App`:

```rust
fn handle_path_picker_outcome(
    &mut self,
    outcome: PathPickerOutcome,
    window: &mut Window,
    cx: &mut Context<Self>,
) {
    match outcome {
        PathPickerOutcome::Picked(path) => self.open_repository_at(path, window, cx),
        PathPickerOutcome::Cancelled => {}
        PathPickerOutcome::Failed(message) => self.push_open_failed(message, window, cx),
    }
}

fn push_open_failed(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
    self.notifications.update(cx, |list, cx| {
        list.push(Notification::error(message.clone()), window, cx);
    });
    cx.emit(OpenFailed(message));
    cx.notify();
}
```

Replace `prompt_and_open_repository` with:

```rust
pub fn prompt_and_open_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let task = self
        .path_picker
        .pick_path(repository_prompt_options(), window, cx);

    cx.spawn_in(window, async move |this, cx| {
        let outcome = task.await;
        this.update_in(cx, |app, window, cx| {
            app.handle_path_picker_outcome(outcome, window, cx);
        })
        .ok();
    })
    .detach();
}
```

In the existing picker-error and repository-error paths, replace duplicated notification code with `self.push_open_failed(message, window, cx)`.

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test --test app_shell prompt_
```

Expected: PASS.

- [ ] **Step 6: Run existing app tests**

Run:

```bash
cargo test app::tests
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs tests/common/mod.rs tests/app_shell.rs
git commit -m "feat(app): inject repository path picker"
```

### Task 5: Dispatch Regression And Action Handler Fix

**Files:**
- Modify: `src/app.rs`
- Modify: `tests/app_shell.rs`

- [ ] **Step 1: Write failing dispatch tests**

Append these tests to `tests/app_shell.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify the regression**

Run:

```bash
cargo test --test app_shell repository_picker
```

Expected: FAIL because the picker is not invoked through gpui dispatch on the current wiring.

- [ ] **Step 3: Put the action handler in the rendered dispatch tree**

Modify `src/app.rs` imports:

```rust
use gpui::{
    actions, div, px, rgb, AppContext, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Render, Styled, Window,
};
```

Modify the outermost element returned by `impl Render for App`:

```rust
div()
    .relative()
    .w_full()
    .h_full()
    .track_focus(&self.focus_handle)
    .on_action(cx.listener(|app, _: &OpenRepository, window, cx| {
        app.prompt_and_open_repository(window, cx);
    }))
    .child(body)
    .child(self.notifications.clone())
```

Keep the `cx.on_next_frame(... window.focus(&app.focus_handle) ...)` call added in Task 4 so the root view participates in key dispatch immediately after first render.

- [ ] **Step 4: Run dispatch tests to verify they pass**

Run:

```bash
cargo test --test app_shell repository_picker
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs tests/app_shell.rs
git commit -m "fix(app): dispatch OpenRepository from the root view"
```

### Task 6: Dispatch-Level Picker Outcomes

**Files:**
- Modify: `tests/app_shell.rs`

- [ ] **Step 1: Write success, invalid-path, and cancellation dispatch tests**

Append these tests to `tests/app_shell.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they pass**

Run:

```bash
cargo test --test app_shell cmd_o_
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/app_shell.rs
git commit -m "test(app): cover open repository picker outcomes"
```

### Task 7: Smoke Test Uses Dispatch

**Files:**
- Modify: `tests/smoke.rs`

- [ ] **Step 1: Update the smoke test to open through `cmd-o`**

Modify `tests/smoke.rs` imports:

```rust
use common::QueuedPathPicker;
use gpui::TestAppContext;
use greviewer::app::{
    bind_app_keys, App, Mode, PathPickerOutcome, OPEN_REPOSITORY_KEYSTROKE,
};
```

Replace `boots_open_repo_renders_head_info` with:

```rust
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
```

Leave `boots_to_the_placeholder` as a boot-construction smoke test.

- [ ] **Step 2: Run smoke tests**

Run:

```bash
cargo test --test smoke
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/smoke.rs
git commit -m "test(app): drive smoke open through dispatch"
```

### Task 8: Final Verification

**Files:**
- No source edits expected

- [ ] **Step 1: Run the full verification command**

Run:

```bash
bin/check
```

Expected: PASS with zero formatter errors, clippy warnings, or test failures.

- [ ] **Step 2: Inspect the final diff and history**

Run:

```bash
git status --short
git log --oneline -8
```

Expected: `git status --short` shows only intentionally untracked local files such as `AGENTS.md`. The recent commits should be the task commits from this plan.

- [ ] **Step 3: Update the handoff status if requested**

Do not edit `status.md` unless the user asks for a new handoff. If they do ask, update it with the final commit list, verification result, and any residual manual-QA gaps.
