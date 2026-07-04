//! Conversation data model: threads, turns, anchors.
//!
//! Threads are plain data with no process or UI coupling so the future
//! "start a review" feature can persist them (ADR-0005).

use std::ops::RangeInclusive;
use std::path::PathBuf;

use uuid::Uuid;

/// Stable identity of one conversation; doubles as the CLI session id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(pub Uuid);

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

/// What kind of task the thread runs (ADR-0005 domain model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadKind {
    Review,
    Ask,
    Summary,
}

/// Which side of a side-by-side diff a selection was made on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    Old,
    New,
}

/// Where in the changeset a conversation is attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub file: PathBuf,
    pub line_range: RangeInclusive<u32>,
    pub side: DiffSide,
    /// The changeset identity the anchor was made against (newest commit sha).
    pub changeset_sha: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub speaker: Speaker,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadStatus {
    Idle,
    Running,
    Failed(String),
    Cancelled,
}

/// One conversation with the AI: transcript plus enough identity to resume it.
#[derive(Debug, Clone)]
pub struct Thread {
    pub id: ThreadId,
    pub kind: ThreadKind,
    pub anchor: Option<Anchor>,
    pub turns: Vec<Turn>,
    pub status: ThreadStatus,
    /// True once the CLI has stored the session, i.e. after the first turn
    /// completes; later turns pass `--resume` instead of `--session-id`.
    pub has_run_once: bool,
    /// Where the thread's turns run; recorded by the session manager on the
    /// first turn so follow-ups can respawn in the same repo.
    pub repo_root: Option<PathBuf>,
}

impl Thread {
    pub fn new(kind: ThreadKind, anchor: Option<Anchor>) -> Self {
        Self {
            id: ThreadId(Uuid::new_v4()),
            kind,
            anchor,
            turns: Vec::new(),
            status: ThreadStatus::Idle,
            has_run_once: false,
            repo_root: None,
        }
    }

    /// The session id handed to the CLI (`--session-id` / `--resume`).
    pub fn session_id(&self) -> String {
        self.id.to_string()
    }

    pub fn push_user_turn(&mut self, text: String) {
        self.turns.push(Turn {
            speaker: Speaker::User,
            text,
        });
    }

    /// Streamed assistant output: extend the trailing assistant turn, or open
    /// one if the transcript doesn't end with an assistant turn.
    pub fn append_assistant_text(&mut self, delta: &str) {
        match self.turns.last_mut() {
            Some(turn) if turn.speaker == Speaker::Assistant => turn.text.push_str(delta),
            _ => self.turns.push(Turn {
                speaker: Speaker::Assistant,
                text: delta.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_thread_is_idle_with_a_session_uuid() {
        let thread = Thread::new(ThreadKind::Summary, None);
        assert_eq!(thread.status, ThreadStatus::Idle);
        assert!(thread.turns.is_empty());
        // Session id must be a hyphenated UUID the CLI accepts for --session-id.
        assert_eq!(thread.session_id().len(), 36);
        assert_eq!(thread.session_id(), thread.id.to_string());
    }

    #[test]
    fn distinct_threads_get_distinct_ids() {
        let a = Thread::new(ThreadKind::Ask, None);
        let b = Thread::new(ThreadKind::Ask, None);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn append_assistant_text_streams_into_one_turn() {
        let mut thread = Thread::new(ThreadKind::Ask, None);
        thread.push_user_turn("why?".to_string());
        thread.append_assistant_text("because ");
        thread.append_assistant_text("reasons");
        assert_eq!(thread.turns.len(), 2);
        assert_eq!(thread.turns[0].speaker, Speaker::User);
        assert_eq!(thread.turns[1].speaker, Speaker::Assistant);
        assert_eq!(thread.turns[1].text, "because reasons");
    }

    #[test]
    fn assistant_text_after_user_turn_opens_a_new_turn() {
        let mut thread = Thread::new(ThreadKind::Ask, None);
        thread.push_user_turn("q1".to_string());
        thread.append_assistant_text("a1");
        thread.push_user_turn("q2".to_string());
        thread.append_assistant_text("a2");
        assert_eq!(thread.turns.len(), 4);
        assert_eq!(thread.turns[3].text, "a2");
    }

    #[test]
    fn anchor_carries_location_and_changeset() {
        let anchor = Anchor {
            file: "src/lib.rs".into(),
            line_range: 10..=20,
            side: DiffSide::New,
            changeset_sha: "abc123".to_string(),
        };
        let thread = Thread::new(ThreadKind::Ask, Some(anchor.clone()));
        assert_eq!(thread.anchor.as_ref().unwrap().file, anchor.file);
    }
}
