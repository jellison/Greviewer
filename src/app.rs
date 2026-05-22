//! Top-level application entity and root view.

use gpui::{
    actions, div, px, rgb, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement,
    PathPromptOptions, Render, Styled, Window,
};
use gpui_component::notification::{Notification, NotificationList};
use std::path::PathBuf;

use crate::repo;

actions!(app, [OpenRepository]);

pub struct App {
    pub mode: Mode,
    notifications: Entity<NotificationList>,
}

pub enum Mode {
    NoRepo,
    RepoOpen { repo: repo::OpenRepository },
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
        let notifications = cx.new(|cx| NotificationList::new(window, cx));

        Self {
            mode: Mode::NoRepo,
            notifications,
        }
    }

    /// Show the OS folder picker. When the user picks a directory, hand the
    /// path off to `open_repository_at`. Cancellations are silent; picker
    /// errors surface through the notification list.
    pub fn prompt_and_open_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });

        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            this.update_in(cx, |app, window, cx| match result {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => {
                    let path = paths.remove(0);
                    app.open_repository_at(path, window, cx);
                }
                Ok(Ok(_)) => {
                    // User cancelled the picker or chose nothing.
                }
                Ok(Err(err)) => {
                    let message = format!("Couldn't open the picker: {err}.");
                    app.notifications.update(cx, |list, cx| {
                        list.push(Notification::error(message.clone()), window, cx);
                    });
                    cx.emit(OpenFailed(message));
                    cx.notify();
                }
                Err(_recv_err) => {
                    // The picker channel was dropped before resolving; treat
                    // as a cancellation.
                }
            })
            .ok();
        })
        .detach();
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
                cx.notify();
            }
            Err(err) => {
                let message = err.to_string();
                self.notifications.update(cx, |list, cx| {
                    list.push(Notification::error(message.clone()), window, cx);
                });
                cx.emit(OpenFailed(message));
                cx.notify();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn notification_count(&self, cx: &gpui::App) -> usize {
        self.notifications.read(cx).notifications().len()
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let body = div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .gap_2();

        let body = match &self.mode {
            Mode::NoRepo => body
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
                ),
            Mode::RepoOpen { repo } => {
                let path_text = repo.path.display().to_string();
                let head_line = match &repo.head {
                    Some(head) => format!("{} · {}", head.short_sha, head.summary),
                    None => "No commits yet.".to_string(),
                };

                body.child(
                    div()
                        .text_color(rgb(0xe6e6e6))
                        .text_size(px(20.))
                        .font_family("monospace")
                        .child(path_text),
                )
                .child(
                    div()
                        .text_color(rgb(0x999999))
                        .text_size(px(14.))
                        .child(head_line),
                )
            }
        };

        div()
            .relative()
            .w_full()
            .h_full()
            .child(body)
            .child(self.notifications.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Mode, OpenFailed};
    use git2::{IndexAddOption, Repository, Signature};
    use gpui::TestAppContext;
    use std::fs;

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
