//! Subprocess handling for headless Claude CLI sessions: command
//! construction, stream-json parsing, and process supervision (ADR-0005).
//! Nothing outside `src/ai` touches a subprocess or a raw JSON event.

use serde_json::Value;

/// The subset of CLI stream-json events the integration reacts to.
/// Everything else — hooks, rate-limit notices, post-turn summaries, and
/// event types future CLI versions invent — is `Ignored`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliEvent {
    /// First event of a session; confirms the CLI accepted our session id.
    Init {
        session_id: String,
    },
    /// A chunk of assistant prose (one `text` content block).
    AssistantText {
        text: String,
    },
    /// The assistant invoked a tool; surfaced so long tasks show life.
    ToolActivity {
        name: String,
    },
    /// Terminal event of a turn.
    Result {
        is_error: bool,
        text: String,
    },
    Ignored,
}

/// Parse one stdout line of `--output-format stream-json` output.
pub fn parse_event(line: &str) -> CliEvent {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return CliEvent::Ignored;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("system") if value.get("subtype").and_then(Value::as_str) == Some("init") => {
            match value.get("session_id").and_then(Value::as_str) {
                Some(id) => CliEvent::Init {
                    session_id: id.to_string(),
                },
                None => CliEvent::Ignored,
            }
        }
        Some("assistant") => parse_assistant(&value),
        Some("result") => CliEvent::Result {
            is_error: value
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            text: value
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        _ => CliEvent::Ignored,
    }
}

fn parse_assistant(value: &Value) -> CliEvent {
    let blocks = value
        .pointer("/message/content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // A message carries one or more content blocks; text blocks are joined,
    // and a tool_use block wins over accompanying (usually empty) text.
    let mut text = String::new();
    for block in &blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                return CliEvent::ToolActivity { name };
            }
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    text.push_str(t);
                }
            }
            _ => {}
        }
    }
    if text.is_empty() {
        CliEvent::Ignored
    } else {
        CliEvent::AssistantText { text }
    }
}

use std::path::PathBuf;
use std::process::Command;

/// Bash patterns the session may run: read-only git inspection only.
/// `git checkout`, `stash`, and anything else mutating is absent by design —
/// with `--permission-mode dontAsk`, absent means denied.
const ALLOWED_TOOLS: &str = "Read Grep Glob \
    Bash(git log*) Bash(git show*) Bash(git diff*) Bash(git status*) \
    Bash(git blame*) Bash(git cat-file*) Bash(git ls-tree*) Bash(git ls-files*) \
    Bash(git rev-parse*) Bash(git rev-list*) Bash(git branch --list*) Bash(git merge-base*)";

const DISALLOWED_TOOLS: &str = "Edit Write NotebookEdit";

/// Everything needed to spawn one conversation turn.
#[derive(Debug, Clone)]
pub struct TurnSpec {
    /// The CLI binary; `claude` resolved via PATH by default, overridable so
    /// tests can substitute a stub script.
    pub program: PathBuf,
    pub repo_root: PathBuf,
    pub session_id: String,
    /// False on a thread's first turn (`--session-id`), true after
    /// (`--resume`) — the CLI's session storage supplies the memory.
    pub resume: bool,
    pub prompt: String,
    /// Structured-output schema for review turns; None for prose turns.
    pub json_schema: Option<String>,
}

/// Build the read-only, settings-isolated headless invocation for a turn.
/// The environment is deliberately inherited untouched: corporate-gateway
/// configuration (`ANTHROPIC_BASE_URL` etc.) flows through it (ADR-0005).
pub fn build_command(spec: &TurnSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.current_dir(&spec.repo_root);
    cmd.arg("-p").arg(&spec.prompt);
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--verbose"); // required by stream-json output
    cmd.arg("--permission-mode").arg("dontAsk");
    cmd.arg("--tools").arg("Read,Grep,Glob,Bash");
    cmd.arg("--allowedTools").arg(ALLOWED_TOOLS);
    cmd.arg("--disallowedTools").arg(DISALLOWED_TOOLS);
    cmd.arg("--setting-sources").arg("");
    if spec.resume {
        cmd.arg("--resume").arg(&spec.session_id);
    } else {
        cmd.arg("--session-id").arg(&spec.session_id);
    }
    if let Some(schema) = &spec.json_schema {
        cmd.arg("--json-schema").arg(schema);
    }
    cmd
}

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// How one turn's subprocess ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    /// The CLI ran to completion and emitted a result event.
    Completed { is_error: bool, text: String },
    /// The turn was cancelled via [`KillHandle::kill`].
    Killed,
    /// The process could not be started (binary missing, not executable…).
    SpawnFailed(String),
    /// The process exited abnormally without a result event; carries stderr.
    ProcessFailed(String),
}

/// Cross-thread cancellation for a running turn. Cloneable so the session
/// manager can keep one and hand copies to kill-all sweeps.
#[derive(Clone)]
pub struct KillHandle {
    child: Arc<Mutex<Option<Child>>>,
    killed: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl KillHandle {
    fn new() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            killed: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Terminate the child. Safe to call from any thread, at any time,
    /// repeatedly. The reader thread reports [`TurnOutcome::Killed`].
    pub fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().expect("kill lock").as_mut() {
            kill_process_group(child);
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

/// Kill the child's whole process group, not just the child: the CLI (or a
/// test stub shell) spawns its own children — Bash tool processes — which
/// inherit the stdout pipe and would otherwise keep the reader thread
/// blocked long after the direct child died. The child is spawned as its own
/// group leader (see `run_turn`), so signalling `-pid` reaches the tree.
fn kill_process_group(child: &mut Child) {
    let pid = child.id();
    Command::new("/bin/kill")
        .args(["-9", "--", &format!("-{pid}")])
        .status()
        .ok();
    // Fallback for the direct child; failure means it already exited.
    child.kill().ok();
}

/// Spawn one turn and stream its events from a dedicated reader thread.
///
/// `on_event` runs on that thread — pass a channel sender, not anything
/// touching gpui state. The join handle yields the turn's outcome.
pub fn run_turn(
    spec: &TurnSpec,
    mut on_event: impl FnMut(CliEvent) + Send + 'static,
) -> (KillHandle, std::thread::JoinHandle<TurnOutcome>) {
    let handle = KillHandle::new();
    let mut cmd = build_command(spec);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    // Own process group so cancellation can kill the CLI *and* its children.
    cmd.process_group(0);

    let thread_handle = handle.clone();
    let join = std::thread::spawn(move || {
        let outcome = run_turn_blocking(cmd, &thread_handle, &mut on_event);
        thread_handle.finished.store(true, Ordering::SeqCst);
        outcome
    });
    (handle, join)
}

fn run_turn_blocking(
    mut cmd: Command,
    handle: &KillHandle,
    on_event: &mut impl FnMut(CliEvent),
) -> TurnOutcome {
    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return TurnOutcome::SpawnFailed(err.to_string()),
    };
    let stdout = {
        let mut guard = handle.child.lock().expect("store child");
        let slot = guard.insert(child);
        // A kill() that raced the spawn saw no child to signal; honor it now.
        if handle.killed.load(Ordering::SeqCst) {
            kill_process_group(slot);
        }
        slot.stdout.take().expect("stdout was piped")
    };

    let mut completed: Option<TurnOutcome> = None;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        match parse_event(&line) {
            CliEvent::Ignored => {}
            CliEvent::Result { is_error, text } => {
                completed = Some(TurnOutcome::Completed {
                    is_error,
                    text: text.clone(),
                });
                on_event(CliEvent::Result { is_error, text });
            }
            event => on_event(event),
        }
    }

    // Stdout closed: reap the child and classify the ending.
    let mut child = handle
        .child
        .lock()
        .expect("take child")
        .take()
        .expect("child was stored");
    let mut stderr_text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        stderr.read_to_string(&mut stderr_text).ok();
    }
    let status = child.wait();

    if handle.killed.load(Ordering::SeqCst) {
        return TurnOutcome::Killed;
    }
    if let Some(outcome) = completed {
        return outcome;
    }
    let status_text = status
        .map(|s| s.to_string())
        .unwrap_or_else(|err| err.to_string());
    TurnOutcome::ProcessFailed(format!("{status_text}: {}", stderr_text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real events captured from `claude -p ... --output-format stream-json
    // --verbose` (Claude CLI 2.1.200), abbreviated only by dropping fields
    // the parser must ignore anyway is NOT done: full lines kept verbatim
    // where feasible, extra fields intact to prove tolerant parsing.

    #[test]
    fn parses_system_init_into_session_id() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/tmp/x","session_id":"8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d","tools":[],"model":"claude-opus-4-8[1m]","permissionMode":"dontAsk","apiKeySource":"ANTHROPIC_AUTH_TOKEN","uuid":"11111111-2222-3333-4444-555555555555"}"#;
        match parse_event(line) {
            CliEvent::Init { session_id } => {
                assert_eq!(session_id, "8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d");
            }
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn parses_assistant_text_blocks() {
        let line = r#"{"type":"assistant","message":{"id":"msg_01","type":"message","role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":1}},"parent_tool_use_id":null,"session_id":"8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d","uuid":"aaa"}"#;
        match parse_event(line) {
            CliEvent::AssistantText { text } => assert_eq!(text, "hello"),
            other => panic!("expected AssistantText, got {other:?}"),
        }
    }

    #[test]
    fn parses_assistant_tool_use_into_tool_activity() {
        let line = r#"{"type":"assistant","message":{"id":"msg_02","type":"message","role":"assistant","model":"claude-opus-4-8","content":[{"type":"tool_use","id":"toolu_01","name":"Bash","input":{"command":"git log --oneline -5"}}],"stop_reason":"tool_use","usage":{}},"session_id":"s","uuid":"bbb"}"#;
        match parse_event(line) {
            CliEvent::ToolActivity { name } => assert_eq!(name, "Bash"),
            other => panic!("expected ToolActivity, got {other:?}"),
        }
    }

    #[test]
    fn parses_result_success() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":4141,"num_turns":1,"result":"hello","session_id":"8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d","total_cost_usd":0.028,"usage":{},"uuid":"ccc"}"#;
        match parse_event(line) {
            CliEvent::Result { is_error, text } => {
                assert!(!is_error);
                assert_eq!(text, "hello");
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn parses_error_result() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"API error","session_id":"s","uuid":"ddd"}"#;
        match parse_event(line) {
            CliEvent::Result { is_error, text } => {
                assert!(is_error);
                assert_eq!(text, "API error");
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn unknown_events_and_garbage_are_ignored() {
        // Real events the integration deliberately ignores…
        let rate_limit =
            r#"{"type":"rate_limit_event","rate_limit_info":{},"session_id":"s","uuid":"e"}"#;
        let post_turn = r#"{"type":"system","subtype":"post_turn_summary","needs_action":false,"session_id":"s","uuid":"f"}"#;
        // …and outright garbage, which must not panic.
        for line in [rate_limit, post_turn, "not json at all", "", "{}"] {
            assert!(
                matches!(parse_event(line), CliEvent::Ignored),
                "line: {line}"
            );
        }
    }

    use std::path::PathBuf;

    fn spec() -> TurnSpec {
        TurnSpec {
            program: PathBuf::from("claude"),
            repo_root: PathBuf::from("/tmp/repo"),
            session_id: "8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d".to_string(),
            resume: false,
            prompt: "Summarize abc..def".to_string(),
            json_schema: None,
        }
    }

    fn args_of(cmd: &std::process::Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn flag_value(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    }

    #[test]
    fn first_turn_assigns_session_id_and_streams_json() {
        let cmd = build_command(&spec());
        let args = args_of(&cmd);
        assert_eq!(cmd.get_program(), "claude");
        assert_eq!(
            cmd.get_current_dir(),
            Some(std::path::Path::new("/tmp/repo"))
        );
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"Summarize abc..def".to_string()));
        assert_eq!(
            flag_value(&args, "--output-format").as_deref(),
            Some("stream-json")
        );
        assert!(args.contains(&"--verbose".to_string()));
        assert_eq!(
            flag_value(&args, "--session-id").as_deref(),
            Some("8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d")
        );
        assert!(!args.contains(&"--resume".to_string()));
    }

    #[test]
    fn follow_up_turns_resume_instead_of_assigning() {
        let cmd = build_command(&TurnSpec {
            resume: true,
            ..spec()
        });
        let args = args_of(&cmd);
        assert_eq!(
            flag_value(&args, "--resume").as_deref(),
            Some("8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d")
        );
        assert!(!args.contains(&"--session-id".to_string()));
    }

    #[test]
    fn sessions_are_read_only_and_isolated() {
        let cmd = build_command(&spec());
        let args = args_of(&cmd);
        // Deny-not-prompt permission mode: headless has no one to ask.
        assert_eq!(
            flag_value(&args, "--permission-mode").as_deref(),
            Some("dontAsk")
        );
        // Only read/search tools plus Bash exist at all…
        assert_eq!(
            flag_value(&args, "--tools").as_deref(),
            Some("Read,Grep,Glob,Bash")
        );
        // …and Bash is allowlisted to read-only git inspection only.
        let allowed = flag_value(&args, "--allowedTools").expect("allowedTools present");
        assert!(allowed.contains("Bash(git log*)"));
        assert!(allowed.contains("Bash(git show*)"));
        assert!(allowed.contains("Bash(git diff*)"));
        assert!(!allowed.contains("git checkout"));
        assert!(!allowed.contains("git stash"));
        // Mutating tools are explicitly denied, belt and suspenders.
        let denied = flag_value(&args, "--disallowedTools").expect("disallowedTools present");
        for tool in ["Edit", "Write", "NotebookEdit"] {
            assert!(denied.contains(tool), "{tool} must be disallowed");
        }
        // Spawned sessions must not load the user's hooks/plugins/settings.
        assert_eq!(flag_value(&args, "--setting-sources").as_deref(), Some(""));
    }

    #[test]
    fn review_turns_request_a_json_schema() {
        let schema = r#"{"type":"object"}"#.to_string();
        let cmd = build_command(&TurnSpec {
            json_schema: Some(schema.clone()),
            ..spec()
        });
        let args = args_of(&cmd);
        assert_eq!(flag_value(&args, "--json-schema"), Some(schema));
    }

    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::mpsc;

    /// Write an executable stub script standing in for the `claude` binary.
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

    fn spec_for(program: PathBuf, repo_root: PathBuf) -> TurnSpec {
        TurnSpec {
            program,
            repo_root,
            ..spec()
        }
    }

    #[test]
    fn run_turn_streams_events_and_completes() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Replays a realistic three-event transcript.
        let program = stub_cli(
            dir.path(),
            r#"echo '{"type":"system","subtype":"init","session_id":"8a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d"}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"hello"}'"#,
        );
        let (events_tx, events_rx) = mpsc::channel();
        let (_kill, join) = run_turn(&spec_for(program, dir.path().to_path_buf()), move |ev| {
            events_tx.send(ev).ok();
        });
        let outcome = join.join().expect("reader thread");
        assert_eq!(
            outcome,
            TurnOutcome::Completed {
                is_error: false,
                text: "hello".to_string()
            }
        );
        let events: Vec<CliEvent> = events_rx.try_iter().collect();
        assert!(matches!(events[0], CliEvent::Init { .. }));
        assert!(matches!(events[1], CliEvent::AssistantText { .. }));
        assert!(matches!(events[2], CliEvent::Result { .. }));
    }

    #[test]
    fn kill_terminates_a_running_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Emits init then hangs; only kill() can end it. The trailing exit
        // stops sh from exec'ing sleep, so the stub keeps the parent+child
        // shape of the real CLI and proves the whole tree is killed.
        let program = stub_cli(
            dir.path(),
            r#"echo '{"type":"system","subtype":"init","session_id":"s"}'
sleep 300
exit 0"#,
        );
        let (events_tx, events_rx) = mpsc::channel();
        let (kill, join) = run_turn(&spec_for(program, dir.path().to_path_buf()), move |ev| {
            events_tx.send(ev).ok();
        });
        // Wait for the init event so we know the process is up.
        events_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("init event");
        assert!(!kill.is_finished());
        kill.kill();
        let outcome = join.join().expect("reader thread");
        assert_eq!(outcome, TurnOutcome::Killed);
        assert!(kill.is_finished());
    }

    #[test]
    fn nonzero_exit_is_a_process_failure_with_stderr() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = stub_cli(
            dir.path(),
            r#"echo 'auth error: bad token' >&2
exit 3"#,
        );
        let (_kill, join) = run_turn(&spec_for(program, dir.path().to_path_buf()), |_| {});
        match join.join().expect("reader thread") {
            TurnOutcome::ProcessFailed(message) => {
                assert!(message.contains("auth error: bad token"), "got: {message}");
            }
            other => panic!("expected ProcessFailed, got {other:?}"),
        }
    }

    #[test]
    fn missing_binary_is_a_spawn_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("no-such-binary");
        let (kill, join) = run_turn(&spec_for(program, dir.path().to_path_buf()), |_| {});
        assert!(matches!(
            join.join().expect("reader thread"),
            TurnOutcome::SpawnFailed(_)
        ));
        assert!(kill.is_finished());
    }
}
