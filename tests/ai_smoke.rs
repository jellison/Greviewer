//! Smoke test (ADR-0003 level 4): the read-only flag set in `ai::cli` is
//! accepted end-to-end by the real installed Claude CLI. Runs only when
//! `GREVIEWER_AI_SMOKE=1` is set and a `claude` binary is on PATH; otherwise
//! it passes as a no-op so `bin/check` is green on unconfigured machines.

use std::path::PathBuf;
use std::sync::mpsc;

use greviewer::ai::cli::{run_turn, CliEvent, TurnOutcome, TurnSpec};

#[test]
fn real_cli_accepts_the_read_only_flag_set() {
    if std::env::var("GREVIEWER_AI_SMOKE").as_deref() != Ok("1") {
        eprintln!("skipping: set GREVIEWER_AI_SMOKE=1 to run the real-CLI smoke test");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = TurnSpec {
        program: PathBuf::from("claude"),
        repo_root: dir.path().to_path_buf(),
        session_id: uuid::Uuid::new_v4().to_string(),
        resume: false,
        prompt: "Reply with exactly: ok".to_string(),
        json_schema: None,
    };
    let (events_tx, events_rx) = mpsc::channel();
    let (_kill, join) = run_turn(&spec, move |ev| {
        events_tx.send(ev).ok();
    });
    let outcome = join.join().expect("reader thread");
    match outcome {
        TurnOutcome::Completed { is_error, text } => {
            assert!(!is_error, "CLI reported an error result: {text}");
        }
        other => panic!("smoke turn did not complete: {other:?}"),
    }
    let events: Vec<CliEvent> = events_rx.try_iter().collect();
    assert!(
        events.iter().any(|e| matches!(e, CliEvent::Init { .. })),
        "no init event — flag set may have been rejected"
    );
}
