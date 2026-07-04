//! AI assistance via headless Claude CLI sessions (ADR-0005).
//!
//! Everything that touches a subprocess or a raw stream-json event lives in
//! this module; the rest of the app sees only threads and typed events.

pub mod cli;
pub mod prompts;
pub mod thread;

use std::collections::HashMap;
use std::path::PathBuf;

use futures::StreamExt as _;
use gpui::{Context, EventEmitter};

use cli::{CliEvent, KillHandle, TurnOutcome, TurnSpec};
pub use thread::{Anchor, DiffSide, Speaker, Thread, ThreadId, ThreadKind, ThreadStatus, Turn};

/// Cap on simultaneously running turns so a click-happy user can't fork-bomb
/// the machine with CLI sessions.
pub const MAX_CONCURRENT_SESSIONS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    AtCapacity,
    ThreadBusy,
    UnknownThread,
}

/// Emitted whenever a thread's transcript or status changes; views subscribe
/// and re-read the thread they care about.
#[derive(Debug, Clone, Copy)]
pub enum AiSessionsEvent {
    ThreadUpdated(ThreadId),
}

/// The one owner of AI threads and their subprocesses. Owned by `App`;
/// nothing outside `src/ai` touches a process or raw event.
pub struct AiSessions {
    cli_program: PathBuf,
    threads: HashMap<ThreadId, Thread>,
    running: HashMap<ThreadId, KillHandle>,
}

impl EventEmitter<AiSessionsEvent> for AiSessions {}

impl AiSessions {
    pub fn new() -> Self {
        Self::with_cli_program(PathBuf::from("claude"))
    }

    /// Test seam: substitute a stub script for the real CLI binary.
    pub fn with_cli_program(cli_program: PathBuf) -> Self {
        Self {
            cli_program,
            threads: HashMap::new(),
            running: HashMap::new(),
        }
    }

    pub fn thread(&self, id: ThreadId) -> Option<&Thread> {
        self.threads.get(&id)
    }

    pub fn threads(&self) -> impl Iterator<Item = &Thread> {
        self.threads.values()
    }

    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    /// Create a thread and run its first turn.
    pub fn start_thread(
        &mut self,
        repo_root: PathBuf,
        kind: ThreadKind,
        anchor: Option<Anchor>,
        prompt: String,
        json_schema: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<ThreadId, StartError> {
        if self.running.len() >= MAX_CONCURRENT_SESSIONS {
            return Err(StartError::AtCapacity);
        }
        let thread = Thread::new(kind, anchor);
        let id = thread.id;
        self.threads.insert(id, thread);
        self.run_turn_for(id, repo_root, prompt, json_schema, cx)?;
        Ok(id)
    }

    /// Run a follow-up turn on an existing, idle thread.
    pub fn send_turn(
        &mut self,
        id: ThreadId,
        prompt: String,
        cx: &mut Context<Self>,
    ) -> Result<(), StartError> {
        let thread = self.threads.get(&id).ok_or(StartError::UnknownThread)?;
        if self.running.contains_key(&id) {
            return Err(StartError::ThreadBusy);
        }
        if self.running.len() >= MAX_CONCURRENT_SESSIONS {
            return Err(StartError::AtCapacity);
        }
        // A follow-up needs the repo the thread was started in; threads keep
        // no repo state, so the anchor-less summary/ask flow re-supplies it
        // at the call site in later feature work. For the foundation, the
        // repo root is the process working directory of the previous turn,
        // which the manager records below.
        let repo_root = thread.repo_root.clone().ok_or(StartError::UnknownThread)?;
        self.run_turn_for(id, repo_root, prompt, None, cx)
    }

    pub fn cancel(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        if let Some(kill) = self.running.remove(&id) {
            kill.kill();
        }
        if let Some(thread) = self.threads.get_mut(&id) {
            thread.status = ThreadStatus::Cancelled;
        }
        cx.emit(AiSessionsEvent::ThreadUpdated(id));
        cx.notify();
    }

    /// Kill every running turn. Invoked on changeset close and app quit —
    /// no orphaned `claude` processes, ever (ADR-0005).
    pub fn cancel_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<ThreadId> = self.running.keys().copied().collect();
        for id in ids {
            self.cancel(id, cx);
        }
    }

    fn run_turn_for(
        &mut self,
        id: ThreadId,
        repo_root: PathBuf,
        prompt: String,
        json_schema: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), StartError> {
        let thread = self.threads.get_mut(&id).ok_or(StartError::UnknownThread)?;
        thread.push_user_turn(prompt.clone());
        thread.status = ThreadStatus::Running;
        thread.repo_root = Some(repo_root.clone());
        let spec = TurnSpec {
            program: self.cli_program.clone(),
            repo_root,
            session_id: thread.session_id(),
            resume: thread.has_run_once,
            prompt,
            json_schema,
        };

        // Bridge: the reader thread pushes events into an async channel; a
        // foreground task drains it and applies them to the entity.
        let (events_tx, mut events_rx) = futures::channel::mpsc::unbounded::<CliEvent>();
        let (kill, join) = cli::run_turn(&spec, move |event| {
            events_tx.unbounded_send(event).ok();
        });
        self.running.insert(id, kill);

        cx.spawn(async move |this, cx| {
            while let Some(event) = events_rx.next().await {
                let applied = this.update(cx, |sessions, cx| {
                    sessions.apply_event(id, &event, cx);
                });
                if applied.is_err() {
                    return; // entity dropped; reader thread ends on its own
                }
            }
            // Channel closed: the process is done. Collect the outcome off
            // the blocking join handle without blocking the foreground.
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    join.join().unwrap_or_else(|_| {
                        TurnOutcome::ProcessFailed("reader thread panicked".to_string())
                    })
                })
                .await;
            this.update(cx, |sessions, cx| sessions.finish_turn(id, outcome, cx))
                .ok();
        })
        .detach();

        cx.emit(AiSessionsEvent::ThreadUpdated(id));
        cx.notify();
        Ok(())
    }

    fn apply_event(&mut self, id: ThreadId, event: &CliEvent, cx: &mut Context<Self>) {
        let Some(thread) = self.threads.get_mut(&id) else {
            return;
        };
        match event {
            CliEvent::AssistantText { text } => thread.append_assistant_text(text),
            // Init confirms the CLI accepted our session id; ToolActivity is
            // carried through for progress UI in later feature work.
            CliEvent::Init { .. } | CliEvent::ToolActivity { .. } => {}
            CliEvent::Result { .. } | CliEvent::Ignored => return,
        }
        cx.emit(AiSessionsEvent::ThreadUpdated(id));
        cx.notify();
    }

    fn finish_turn(&mut self, id: ThreadId, outcome: TurnOutcome, cx: &mut Context<Self>) {
        self.running.remove(&id);
        let Some(thread) = self.threads.get_mut(&id) else {
            return;
        };
        // A cancel that raced process exit stays Cancelled.
        if thread.status == ThreadStatus::Cancelled {
            return;
        }
        thread.status = match outcome {
            TurnOutcome::Completed {
                is_error: false, ..
            } => {
                thread.has_run_once = true;
                ThreadStatus::Idle
            }
            TurnOutcome::Completed {
                is_error: true,
                text,
            } => ThreadStatus::Failed(text),
            TurnOutcome::Killed => ThreadStatus::Cancelled,
            TurnOutcome::SpawnFailed(message) | TurnOutcome::ProcessFailed(message) => {
                ThreadStatus::Failed(message)
            }
        };
        cx.emit(AiSessionsEvent::ThreadUpdated(id));
        cx.notify();
    }
}

impl Default for AiSessions {
    fn default() -> Self {
        Self::new()
    }
}

/// Backstop against orphaned `claude` processes: whatever path drops the
/// manager (window close, app teardown) kills every running turn, even if
/// no explicit cancel_all ran first (ADR-0005: no orphans, ever).
impl Drop for AiSessions {
    fn drop(&mut self) {
        for kill in self.running.values() {
            kill.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;

    fn stub_cli(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("claude-stub.sh");
        let mut file = std::fs::File::create(&path).expect("create stub");
        writeln!(file, "#!/bin/sh").expect("write shebang");
        writeln!(file, "{body}").expect("write body");
        let mut perms = file.metadata().expect("stat").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
        path
    }

    const HAPPY_TRANSCRIPT: &str = r#"echo '{"type":"system","subtype":"init","session_id":"stub"}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"hi there"}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"hi there"}'"#;

    /// Poll the entity until `predicate` holds or the deadline passes. The
    /// stub process produces output in real time, but gpui's test timers run
    /// on a fake clock that never advances while the executor is parked — so
    /// wall-clock waiting must be a real sleep between executor turns.
    fn wait_until(
        cx: &mut TestAppContext,
        sessions: &gpui::Entity<AiSessions>,
        predicate: impl Fn(&AiSessions) -> bool,
    ) {
        for _ in 0..200 {
            cx.run_until_parked();
            if sessions.read_with(cx, |s, _| predicate(s)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("condition not reached within 10s");
    }

    #[gpui::test]
    fn thread_runs_to_completion_and_streams_text(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = stub_cli(dir.path(), HAPPY_TRANSCRIPT);
        let sessions = cx.new(|_| AiSessions::with_cli_program(program));

        let id = sessions
            .update(cx, |sessions, cx| {
                sessions.start_thread(
                    dir.path().to_path_buf(),
                    ThreadKind::Summary,
                    None,
                    "Summarize abc..def".to_string(),
                    None,
                    cx,
                )
            })
            .expect("start_thread");

        sessions.read_with(cx, |sessions, _| {
            let thread = sessions.thread(id).expect("thread exists");
            assert_eq!(thread.status, ThreadStatus::Running);
            assert_eq!(thread.turns.len(), 1); // the user prompt
        });

        wait_until(cx, &sessions, |s| {
            s.thread(id).is_some_and(|t| t.status == ThreadStatus::Idle)
        });

        sessions.read_with(cx, |sessions, _| {
            let thread = sessions.thread(id).expect("thread exists");
            assert!(thread.has_run_once);
            assert_eq!(
                thread.turns.last().expect("assistant turn").text,
                "hi there"
            );
        });
    }

    #[gpui::test]
    fn cancel_all_kills_running_threads(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        // Trailing exit keeps sh as the parent of sleep (no exec), matching
        // the real CLI's parent+child process shape.
        let program = stub_cli(
            dir.path(),
            r#"echo '{"type":"system","subtype":"init","session_id":"stub"}'
sleep 300
exit 0"#,
        );
        let sessions = cx.new(|_| AiSessions::with_cli_program(program));
        let id = sessions
            .update(cx, |sessions, cx| {
                sessions.start_thread(
                    dir.path().to_path_buf(),
                    ThreadKind::Ask,
                    None,
                    "hello?".to_string(),
                    None,
                    cx,
                )
            })
            .expect("start_thread");

        sessions.update(cx, |sessions, cx| sessions.cancel_all(cx));

        wait_until(cx, &sessions, |s| {
            s.thread(id)
                .is_some_and(|t| t.status == ThreadStatus::Cancelled)
                && s.running_count() == 0
        });
    }

    #[gpui::test]
    fn capacity_is_enforced(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = stub_cli(dir.path(), "sleep 300\nexit 0");
        let sessions = cx.new(|_| AiSessions::with_cli_program(program));
        for _ in 0..MAX_CONCURRENT_SESSIONS {
            sessions
                .update(cx, |sessions, cx| {
                    sessions.start_thread(
                        dir.path().to_path_buf(),
                        ThreadKind::Ask,
                        None,
                        "q".to_string(),
                        None,
                        cx,
                    )
                })
                .expect("start under cap");
        }
        let over = sessions.update(cx, |sessions, cx| {
            sessions.start_thread(
                dir.path().to_path_buf(),
                ThreadKind::Ask,
                None,
                "one too many".to_string(),
                None,
                cx,
            )
        });
        assert!(matches!(over, Err(StartError::AtCapacity)));
        sessions.update(cx, |sessions, cx| sessions.cancel_all(cx));
    }

    #[gpui::test]
    fn spawn_failure_marks_thread_failed(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let sessions = cx.new(|_| AiSessions::with_cli_program(dir.path().join("missing-binary")));
        let id = sessions
            .update(cx, |sessions, cx| {
                sessions.start_thread(
                    dir.path().to_path_buf(),
                    ThreadKind::Ask,
                    None,
                    "q".to_string(),
                    None,
                    cx,
                )
            })
            .expect("start_thread");
        wait_until(cx, &sessions, |s| {
            s.thread(id)
                .is_some_and(|t| matches!(t.status, ThreadStatus::Failed(_)))
        });
    }

    #[gpui::test]
    fn busy_thread_rejects_a_second_turn(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = stub_cli(dir.path(), "sleep 300\nexit 0");
        let sessions = cx.new(|_| AiSessions::with_cli_program(program));
        let id = sessions
            .update(cx, |sessions, cx| {
                sessions.start_thread(
                    dir.path().to_path_buf(),
                    ThreadKind::Ask,
                    None,
                    "q1".to_string(),
                    None,
                    cx,
                )
            })
            .expect("start_thread");
        let second = sessions.update(cx, |sessions, cx| {
            sessions.send_turn(id, "q2".to_string(), cx)
        });
        assert!(matches!(second, Err(StartError::ThreadBusy)));
        sessions.update(cx, |sessions, cx| sessions.cancel_all(cx));
    }
}
