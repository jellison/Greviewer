//! Top-level application entity and root view.

pub mod menu;
pub mod path_picker;

pub use menu::{
    bind_app_keys, build_app_menus, open_repository_key_binding, MenuSnapshot,
    GREVIEWER_MENU_LABEL, OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
};
pub use path_picker::{repository_prompt_options, GpuiPathPicker, PathPicker, PathPickerOutcome};

use gpui::prelude::FluentBuilder;
use gpui::{
    actions, div, px, rgb, AnyElement, AppContext, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled,
    Window,
};
use gpui_component::notification::{Notification, NotificationList};
use std::path::PathBuf;

use crate::repo;

actions!(app, [OpenRepository, OpenChangeset, CloseChangeset]);

pub struct App {
    pub mode: Mode,
    pub selection: Selection,
    pub review_screen: ReviewScreen,
    notifications: Entity<NotificationList>,
    path_picker: Box<dyn PathPicker>,
    focus_handle: FocusHandle,
}

pub enum Mode {
    NoRepo,
    RepoOpen { repo: repo::OpenRepository },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Single { sha: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewScreen {
    Graph,
    Changeset { sha: String },
}

/// Emitted whenever `open_repository_at` fails. Carries the user-facing
/// message that was pushed onto the notification list. Tests subscribe to
/// this event to verify the error path because `gpui-component`'s
/// `Notification` keeps its message field private.
#[derive(Debug, Clone)]
pub struct OpenFailed(pub String);

impl EventEmitter<OpenFailed> for App {}

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

        window.focus(&focus_handle);
        cx.on_next_frame(window, |app, window, _cx| {
            window.focus(&app.focus_handle);
        });

        Self {
            mode: Mode::NoRepo,
            selection: Selection::None,
            review_screen: ReviewScreen::Graph,
            notifications,
            path_picker,
            focus_handle,
        }
    }

    /// Show the OS folder picker. When the user picks a directory, hand the
    /// path off to `open_repository_at`. Cancellations are silent; picker
    /// errors surface through the notification list.
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

    pub fn open_repository_at(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match repo::open_at(&path) {
            Ok(repo) => {
                self.mode = Mode::RepoOpen { repo };
                self.selection = Selection::None;
                self.review_screen = ReviewScreen::Graph;
                cx.notify();
            }
            Err(err) => {
                let message = err.to_string();
                self.push_open_failed(message, window, cx);
            }
        }
    }

    fn select_single_commit(&mut self, sha: String, cx: &mut Context<Self>) {
        self.selection = match &self.selection {
            Selection::Single { sha: selected_sha } if selected_sha == &sha => Selection::None,
            _ => Selection::Single { sha },
        };
        cx.notify();
    }

    fn open_changeset(&mut self, cx: &mut Context<Self>) {
        if let Selection::Single { sha } = &self.selection {
            self.review_screen = ReviewScreen::Changeset { sha: sha.clone() };
            cx.notify();
        }
    }

    fn close_changeset(&mut self, cx: &mut Context<Self>) {
        self.review_screen = ReviewScreen::Graph;
        cx.notify();
    }

    fn is_commit_selected(&self, sha: &str) -> bool {
        matches!(&self.selection, Selection::Single { sha: selected_sha } if selected_sha == sha)
    }

    #[cfg(test)]
    pub(crate) fn notification_count(&self, cx: &gpui::App) -> usize {
        self.notifications.read(cx).notifications().len()
    }

    fn render_no_repo(&self) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(20.))
                    .child("No repository open"),
            )
            .child(
                div()
                    .text_color(rgb(0x999999))
                    .text_size(px(14.))
                    .child("Open a repository to start a review."),
            )
    }

    fn render_repo_open(&self, repo: &repo::OpenRepository, cx: &mut Context<Self>) -> AnyElement {
        match &self.review_screen {
            ReviewScreen::Graph => self.render_graph_screen(repo, cx).into_any_element(),
            ReviewScreen::Changeset { sha } => self
                .render_changeset_screen(repo, sha, cx)
                .into_any_element(),
        }
    }

    fn render_graph_screen(
        &self,
        repo: &repo::OpenRepository,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let path_text = repo.path.display().to_string();
        let head_line = match &repo.head {
            Some(head) => format!("{} · {}", head.short_sha, head.summary),
            None => "No commits yet.".to_string(),
        };
        let can_open_changeset = matches!(self.selection, Selection::Single { .. });

        let title_row = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .child(
                div()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(16.))
                    .font_family("monospace")
                    .child(path_text),
            )
            .when(can_open_changeset, |row| {
                row.child(
                    div()
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x3b82f6))
                        .bg(rgb(0x1d283a))
                        .text_color(rgb(0xdbeafe))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .id("open-changeset")
                        .debug_selector(|| "open-changeset".to_string())
                        .on_click(cx.listener(|app, _event, _window, cx| {
                            app.open_changeset(cx);
                        }))
                        .child("Open changeset"),
                )
            });

        let header = div()
            .flex()
            .flex_col()
            .w_full()
            .gap_1()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x2a2a2a))
            .child(title_row)
            .child(
                div()
                    .text_color(rgb(0x999999))
                    .text_size(px(13.))
                    .child(head_line),
            );

        let history = if repo.commits.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .id("commit-history-empty")
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("This repository has no commits to review.")
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .id("commit-history")
                .overflow_y_scroll()
                .children(
                    repo.commits
                        .iter()
                        .enumerate()
                        .map(|(index, commit)| {
                            self.render_commit_row(
                                index,
                                commit,
                                self.is_commit_selected(&commit.sha),
                                cx,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(rgb(0x171717))
            .child(header)
            .child(history)
    }

    fn render_changeset_screen(
        &self,
        repo: &repo::OpenRepository,
        sha: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let path_text = repo.path.display().to_string();
        let short_sha: String = sha.chars().take(7).collect();

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x2a2a2a))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_color(rgb(0xe6e6e6))
                            .text_size(px(16.))
                            .font_family("monospace")
                            .child(path_text),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x999999))
                            .text_size(px(13.))
                            .child(format!("Changeset for {short_sha}")),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0x4a4a4a))
                    .bg(rgb(0x242424))
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .id("close-changeset")
                    .debug_selector(|| "close-changeset".to_string())
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.close_changeset(cx);
                    }))
                    .child("Close"),
            );

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(rgb(0x171717))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0x999999))
                    .text_size(px(14.))
                    .child("Changed files will appear here in the next slice."),
            )
    }

    fn render_commit_row(
        &self,
        index: usize,
        commit: &repo::CommitInfo,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let marker = if commit.is_head { "HEAD" } else { "" };
        let secondary = format!("{} · {}", commit.author, commit.authored_date);
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
        };
        let border_color = if selected {
            rgb(0x3b82f6)
        } else {
            rgb(0x242424)
        };
        let sha = commit.sha.clone();

        div()
            .flex()
            .items_center()
            .w_full()
            .gap_3()
            .px_4()
            .py_2()
            .bg(row_bg)
            .border_b_1()
            .border_color(border_color)
            .cursor_pointer()
            .id(("commit-row", index))
            .debug_selector(move || format!("commit-row-{index}"))
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.select_single_commit(sha.clone(), cx);
            }))
            .child(
                div()
                    .w(px(48.))
                    .text_color(if commit.is_head {
                        rgb(0x7dd3fc)
                    } else {
                        rgb(0x555555)
                    })
                    .text_size(px(11.))
                    .font_family("monospace")
                    .child(marker),
            )
            .child(
                div()
                    .w(px(72.))
                    .text_color(rgb(0xa3e635))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .child(commit.short_sha.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_1()
                    .child(
                        div()
                            .text_color(rgb(0xe6e6e6))
                            .text_size(px(14.))
                            .child(commit.summary.clone()),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x8a8a8a))
                            .text_size(px(12.))
                            .child(secondary),
                    ),
            )
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.mode {
            Mode::NoRepo => self.render_no_repo().into_any_element(),
            Mode::RepoOpen { repo } => self.render_repo_open(repo, cx),
        };

        div()
            .relative()
            .w_full()
            .h_full()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &OpenRepository, window, cx| {
                app.prompt_and_open_repository(window, cx);
            }))
            .on_action(cx.listener(|app, _: &OpenChangeset, _window, cx| {
                app.open_changeset(cx);
            }))
            .on_action(cx.listener(|app, _: &CloseChangeset, _window, cx| {
                app.close_changeset(cx);
            }))
            .child(body)
            .child(self.notifications.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{App, CloseChangeset, Mode, OpenChangeset, OpenFailed, ReviewScreen, Selection};
    use git2::{IndexAddOption, Repository, Signature};
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use std::fs;

    fn init_repo_with_one_commit() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");

        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let sig =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "Add hello.txt", &tree, &[])
            .expect("create commit");

        drop(tree);
        drop(index);
        drop(repo);

        (dir, oid.to_string())
    }

    #[gpui::test]
    async fn renders_placeholder(cx: &mut TestAppContext) {
        let _window = cx.add_window(App::new);
        // The contract: booting the App in the gpui test harness must not panic
        // and the entity must construct successfully. Once the App gains user-
        // visible interactivity, this test grows to assert on observable events.
    }

    #[gpui::test]
    async fn opening_a_real_repo_advances_to_repo_open_mode(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().to_path_buf();

        let repo = Repository::init(&path).expect("init repo");
        fs::write(path.join("hello.txt"), "hello\n").expect("write file");
        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let sig =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
        repo.commit(Some("HEAD"), &sig, &sig, "Add hello.txt", &tree, &[])
            .expect("create commit");
        drop(tree);
        drop(index);
        drop(repo);

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
                    assert_eq!(head.summary, "Add hello.txt");
                }
                Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
            })
            .expect("read window");
    }

    #[gpui::test]
    async fn selecting_commits_toggles_single_selection(cx: &mut TestAppContext) {
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, _window, cx| {
                app.select_single_commit("first-sha".to_string(), cx);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "first-sha".to_string(),
                    },
                );

                app.select_single_commit("second-sha".to_string(), cx);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "second-sha".to_string(),
                    },
                );

                app.select_single_commit("second-sha".to_string(), cx);
                assert_eq!(app.selection, Selection::None);
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn opening_changeset_requires_a_selection(cx: &mut TestAppContext) {
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, _window, cx| {
                app.open_changeset(cx);
                assert_eq!(app.review_screen, ReviewScreen::Graph);

                app.select_single_commit("selected-sha".to_string(), cx);
                app.open_changeset(cx);
                assert_eq!(
                    app.review_screen,
                    ReviewScreen::Changeset {
                        sha: "selected-sha".to_string(),
                    },
                );
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn closing_changeset_returns_to_graph_and_preserves_selection(cx: &mut TestAppContext) {
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, _window, cx| {
                app.select_single_commit("selected-sha".to_string(), cx);
                app.open_changeset(cx);
                app.close_changeset(cx);

                assert_eq!(app.review_screen, ReviewScreen::Graph);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "selected-sha".to_string(),
                    },
                );
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn dispatching_open_and_close_changeset_actions_updates_review_screen(
        cx: &mut TestAppContext,
    ) {
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, _window, cx| {
                app.select_single_commit("selected-sha".to_string(), cx);
            })
            .expect("select commit");

        cx.dispatch_action(*window, OpenChangeset);

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.review_screen,
                    ReviewScreen::Changeset {
                        sha: "selected-sha".to_string(),
                    },
                );
            })
            .expect("read open changeset state");

        cx.dispatch_action(*window, CloseChangeset);

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.review_screen, ReviewScreen::Graph);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "selected-sha".to_string(),
                    },
                );
            })
            .expect("read closed changeset state");
    }

    #[gpui::test]
    async fn clicking_changeset_affordances_enters_and_exits_review_mode(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex.clone(), cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");

        visual.simulate_click(open_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.review_screen,
                    ReviewScreen::Changeset {
                        sha: oid_hex.clone(),
                    },
                );
            })
            .expect("read opened review state");

        let close_bounds = visual
            .debug_bounds("close-changeset")
            .expect("close changeset debug bounds");

        visual.simulate_click(close_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.review_screen, ReviewScreen::Graph);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: oid_hex.clone(),
                    },
                );
            })
            .expect("read closed review state");
    }

    #[gpui::test]
    async fn clicking_a_commit_row_toggles_selection(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("commit-row-0")
            .expect("commit row debug bounds");

        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: oid_hex.clone(),
                    },
                );
            })
            .expect("read selected state");

        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.selection, Selection::None);
            })
            .expect("read cleared state");
    }

    #[gpui::test]
    async fn opening_a_non_repo_stays_in_no_repo_mode_and_emits_a_notification(
        cx: &mut TestAppContext,
    ) {
        use std::cell::RefCell;
        use std::rc::Rc;

        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().to_path_buf();

        // The notification path renders gpui-component widgets that look up
        // the active theme; the theme global is installed by `gpui_component::init`.
        cx.update(gpui_component::init);

        let window = cx.add_window(App::new);
        let app_entity = window.entity(cx).expect("get app entity");

        // Subscribe to the App's `OpenFailed` event before triggering the open;
        // `Notification` keeps its message private, so this event is the only
        // public observation point for the user-facing string.
        let captured: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let captured_clone = captured.clone();
        let _subscription = app_entity.update(cx, |_, cx| {
            cx.subscribe(&app_entity, move |_, _, event: &OpenFailed, _| {
                captured_clone.borrow_mut().push(event.0.clone());
            })
        });

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("update window");

        cx.run_until_parked();

        window
            .read_with(cx, |app, cx| {
                assert!(matches!(app.mode, Mode::NoRepo), "mode unchanged");
                assert_eq!(
                    app.notification_count(cx),
                    1,
                    "exactly one notification pushed",
                );
            })
            .expect("read window");

        let events = captured.borrow();
        assert_eq!(events.len(), 1, "exactly one OpenFailed event emitted");
        assert!(
            events[0].contains("isn't a Git repository"),
            "notification body matches NotARepository, got {:?}",
            events[0],
        );
    }
}
