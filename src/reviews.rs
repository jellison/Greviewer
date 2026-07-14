//! Persisted reviews (see docs/specs/review/persistence.md).
//!
//! One JSON document per review under `reviews/` in the app data directory,
//! following the `settings` module's forgiving-serde idioms: unknown fields
//! are ignored, missing fields default, and a file that fails to parse is
//! skipped at load — never deleted — so one corrupt review cannot take the
//! rest down. Every mutation writes through immediately; callers surface
//! write errors but keep the in-memory change.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Identity of the changeset a review is anchored to. Full 40-character
/// shas. `Range::start_sha` is the oldest commit, `end_sha` the newest.
/// Comparison direction is preserved: swapping base and target is a
/// different changeset because its content differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangesetKey {
    Single {
        sha: String,
    },
    Range {
        start_sha: String,
        end_sha: String,
    },
    Compare {
        base_sha: String,
        target_sha: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    #[default]
    Active,
    Completed,
}

/// One entry in a guide's suggested reading order. `path` is repo-relative,
/// exactly as the changeset lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewGuideEntry {
    pub path: String,
    pub note: String,
}

/// An AI-generated review guide (docs/specs/ai/review-guide.md). Immutable
/// once generated — regeneration replaces the whole value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewGuide {
    pub summary: String,
    pub review_order: Vec<ReviewGuideEntry>,
    /// Unix seconds, like `Review::created_at`.
    pub generated_at: i64,
}

/// Which side of a diff a thread anchor points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadSide {
    Old,
    #[default]
    New,
}

/// Where a thread is pinned: a text range on one side of a file's diff.
/// Lines are 1-based file line numbers on `side`; columns are UTF-8 byte
/// offsets. `quoted_text` is the exact text the range covered when the
/// thread was started, kept as the re-resolution fallback when line
/// numbers drift.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadAnchor {
    pub side: ThreadSide,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub quoted_text: String,
}

/// Who authored a thread message: the human reviewer, or (from Task 4) an AI
/// agent replying inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageAuthor {
    #[default]
    Reviewer,
    Agent,
}

/// What kind of thread this is. A plain reviewer `Note` (the default, so
/// every document written before Task 4 loads unchanged), or an `Agent`
/// thread whose reviewer questions are answered by the AI (see the "Agent
/// threads" section of docs/specs/review/threads.md). Only the kind and the
/// messages persist; the live CLI session backing an agent thread is
/// transient and rebuilt on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewThreadKind {
    #[default]
    Note,
    Agent,
}

/// One message in a thread's conversation. `ReviewThread::messages` holds
/// these oldest first; a thread's `created_at` is the first message's
/// timestamp, kept as its own field for backward compatibility with reviews
/// persisted before threads carried more than one message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadMessage {
    pub id: String,
    pub author: MessageAuthor,
    pub body: String,
    pub created_at: i64,
}

/// One reviewer thread, anchored to a diff range. Travels with the review
/// document exactly like the guide does. A thread is a conversation of one
/// or more messages, oldest first; `normalize` folds documents persisted
/// before replies existed (a single top-level `body`) into one Reviewer
/// message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewThread {
    pub id: String,
    pub path: String,
    pub anchor: ThreadAnchor,
    /// Whether this is a plain reviewer note or an AI agent thread. Defaults
    /// to `Note`, so documents persisted before Task 4 load unchanged.
    pub kind: ReviewThreadKind,
    /// Oldest first. Never empty after `normalize` has run on a
    /// successfully loaded thread.
    pub messages: Vec<ThreadMessage>,
    /// Legacy single-message body, from before threads carried a
    /// conversation. Read-only: `normalize` folds it into `messages` and
    /// clears it; nothing writes it going forward. `pub(crate)` rather than
    /// public so callers outside this module can't read the stale field,
    /// while still naming it in `..Default::default()` struct updates (this
    /// is a single-binary crate — see ADR-0002 — so `pub(crate)` is the
    /// whole application, not a public API surface).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) body: String,
    pub created_at: i64,
}

impl ReviewThread {
    /// Folds a legacy single-body document into `messages`. Idempotent and
    /// a no-op once `messages` is non-empty. Called on every load path so
    /// old review files keep working losslessly.
    pub fn normalize(&mut self) {
        if self.messages.is_empty() && !self.body.is_empty() {
            self.messages.push(ThreadMessage {
                id: uuid::Uuid::new_v4().to_string(),
                author: MessageAuthor::Reviewer,
                body: self.body.clone(),
                created_at: self.created_at,
            });
            self.body.clear();
        }
    }

    /// When the thread last saw activity: the latest of the thread's own
    /// `created_at` and its messages' timestamps (so a thread with no
    /// messages, or only older-stamped messages, falls back to its own
    /// creation time).
    pub fn last_activity_at(&self) -> i64 {
        self.messages
            .iter()
            .map(|message| message.created_at)
            .fold(self.created_at, i64::max)
    }

    /// The thread's first (oldest) message, if any.
    pub fn first_message(&self) -> Option<&ThreadMessage> {
        self.messages.first()
    }
}

/// One persisted review. Future artifact collections (threads, AI threads,
/// todos) become new fields on this same document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Review {
    pub id: String,
    /// Canonicalized primary-worktree root of the repository under review.
    pub repo_path: PathBuf,
    pub changeset: ChangesetKey,
    pub name: String,
    /// Unix seconds, like `CommitInfo::authored_timestamp`.
    pub created_at: i64,
    pub last_activity_at: i64,
    pub status: ReviewStatus,
    pub completed_at: Option<i64>,
    /// AI-generated review guide, if one has been generated for this changeset.
    pub guide: Option<ReviewGuide>,
    /// Reviewer threads anchored to diff ranges, oldest first.
    #[serde(alias = "comments")]
    pub threads: Vec<ReviewThread>,
}

impl Default for Review {
    fn default() -> Self {
        Self {
            id: String::new(),
            repo_path: PathBuf::new(),
            changeset: ChangesetKey::Single { sha: String::new() },
            name: String::new(),
            created_at: 0,
            last_activity_at: 0,
            status: ReviewStatus::Active,
            completed_at: None,
            guide: None,
            threads: Vec::new(),
        }
    }
}

impl Review {
    pub fn new(repo_path: PathBuf, changeset: ChangesetKey, name: String, now: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            repo_path,
            changeset,
            name,
            created_at: now,
            last_activity_at: now,
            status: ReviewStatus::Active,
            completed_at: None,
            guide: None,
            threads: Vec::new(),
        }
    }
}

/// All loaded reviews plus the directory they persist to. `dir: None` (no
/// resolvable home, or tests) keeps the store purely in memory.
pub struct ReviewStore {
    dir: Option<PathBuf>,
    reviews: Vec<Review>,
}

impl ReviewStore {
    /// Load every parseable `*.json` under `dir`. Missing directory means an
    /// empty store; unparseable files are skipped and left untouched.
    pub fn load(dir: Option<PathBuf>) -> Self {
        let mut reviews = Vec::new();
        if let Some(dir) = &dir {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                        continue;
                    }
                    let Ok(content) = fs::read_to_string(&path) else {
                        continue;
                    };
                    let Ok(mut review) = serde_json::from_str::<Review>(&content) else {
                        continue;
                    };
                    for thread in &mut review.threads {
                        thread.normalize();
                    }
                    reviews.push(review);
                }
            }
        }
        Self { dir, reviews }
    }

    pub fn get(&self, id: &str) -> Option<&Review> {
        self.reviews.iter().find(|review| review.id == id)
    }

    pub fn find_by_changeset(&self, repo_path: &Path, key: &ChangesetKey) -> Option<&Review> {
        self.reviews
            .iter()
            .find(|review| review.repo_path == repo_path && &review.changeset == key)
    }

    /// The repository's reviews for display: active first, most recent
    /// activity first, then completed in the same order.
    pub fn for_repo(&self, repo_path: &Path) -> Vec<&Review> {
        let mut matching: Vec<&Review> = self
            .reviews
            .iter()
            .filter(|review| review.repo_path == repo_path)
            .collect();
        matching.sort_by_key(|review| {
            (
                review.status == ReviewStatus::Completed,
                std::cmp::Reverse(review.last_activity_at),
            )
        });
        matching
    }

    pub fn insert(&mut self, review: Review) -> io::Result<()> {
        let result = self.save(&review);
        self.reviews.push(review);
        result
    }

    /// Apply `apply` to the review with `id` (Ok(false) when unknown), then
    /// persist. The in-memory change sticks even when the save fails, so the
    /// UI stays consistent and the error can be surfaced separately.
    pub fn mutate(&mut self, id: &str, apply: impl FnOnce(&mut Review)) -> io::Result<bool> {
        let Some(review) = self.reviews.iter_mut().find(|review| review.id == id) else {
            return Ok(false);
        };
        apply(review);
        let review = review.clone();
        self.save(&review).map(|()| true)
    }

    pub fn delete(&mut self, id: &str) -> io::Result<bool> {
        let Some(index) = self.reviews.iter().position(|review| review.id == id) else {
            return Ok(false);
        };
        self.reviews.remove(index);
        if let Some(dir) = &self.dir {
            let path = dir.join(format!("{id}.json"));
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(true)
    }

    fn save(&self, review: &Review) -> io::Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        fs::create_dir_all(dir)?;
        let content = serde_json::to_string_pretty(review)
            .map_err(|err| io::Error::other(err.to_string()))?;
        fs::write(dir.join(format!("{}.json", review.id)), content)
    }
}

/// `reviews/` beside `settings.json`. `None` when the platform exposes no
/// config directory, and always `None` under test (mirrors
/// `settings::default_store_path`).
pub fn default_store_dir() -> Option<PathBuf> {
    settings_parent().map(|parent| parent.join("reviews"))
}

#[cfg(test)]
fn settings_parent() -> Option<PathBuf> {
    None
}

#[cfg(not(test))]
fn settings_parent() -> Option<PathBuf> {
    crate::settings::default_store_path().and_then(|path| path.parent().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn key_single() -> ChangesetKey {
        ChangesetKey::Single {
            sha: "a".repeat(40),
        }
    }

    fn sample(dir_path: &Path, name: &str, now: i64) -> Review {
        Review::new(dir_path.to_path_buf(), key_single(), name.to_string(), now)
    }

    #[test]
    fn insert_then_load_round_trips_a_review() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = dir.path().join("repo");
        let mut store = ReviewStore::load(Some(dir.path().join("reviews")));
        let review = sample(&repo, "First pass", 100);
        let id = review.id.clone();
        store.insert(review.clone()).expect("insert");

        let reloaded = ReviewStore::load(Some(dir.path().join("reviews")));
        assert_eq!(reloaded.get(&id), Some(&review));
        assert_eq!(
            reloaded.find_by_changeset(&repo, &key_single()),
            Some(&review)
        );
    }

    #[test]
    fn ids_are_unique_and_name_the_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        let mut store = ReviewStore::load(Some(reviews_dir.clone()));
        let a = sample(&dir.path().join("r1"), "a", 1);
        let b = sample(&dir.path().join("r2"), "b", 2);
        assert_ne!(a.id, b.id);
        store.insert(a.clone()).expect("insert a");
        assert!(reviews_dir.join(format!("{}.json", a.id)).is_file());
    }

    #[test]
    fn corrupt_files_are_skipped_and_left_on_disk() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        let mut store = ReviewStore::load(Some(reviews_dir.clone()));
        store
            .insert(sample(&dir.path().join("repo"), "good", 5))
            .expect("insert");
        let corrupt = reviews_dir.join("corrupt.json");
        fs::write(&corrupt, "not json").expect("write corrupt");

        let reloaded = ReviewStore::load(Some(reviews_dir.clone()));
        assert_eq!(reloaded.for_repo(&dir.path().join("repo")).len(), 1);
        // Skip must never destroy user data.
        assert!(corrupt.is_file());
    }

    #[test]
    fn unknown_fields_load_and_missing_optional_fields_default() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        fs::create_dir_all(&reviews_dir).expect("mkdir");
        // A file from a future schema version: extra field, no completed_at.
        let json = format!(
            r#"{{"id":"x1","repo_path":"/r","changeset":{{"kind":"single","sha":"{}"}},
                "name":"n","created_at":1,"last_activity_at":2,"status":"active",
                "future_field":true}}"#,
            "b".repeat(40)
        );
        fs::write(reviews_dir.join("x1.json"), json).expect("write");

        let store = ReviewStore::load(Some(reviews_dir));
        let review = store.get("x1").expect("loads");
        assert_eq!(review.completed_at, None);
        assert_eq!(review.status, ReviewStatus::Active);
    }

    #[test]
    fn compare_keys_are_directional() {
        let ab = ChangesetKey::Compare {
            base_sha: "a".repeat(40),
            target_sha: "b".repeat(40),
        };
        let ba = ChangesetKey::Compare {
            base_sha: "b".repeat(40),
            target_sha: "a".repeat(40),
        };
        assert_ne!(ab, ba);
    }

    #[test]
    fn for_repo_orders_active_by_activity_then_completed() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = dir.path().join("repo");
        let mut store = ReviewStore::load(Some(dir.path().join("reviews")));
        let mut old_active = Review::new(
            repo.clone(),
            ChangesetKey::Single {
                sha: "1".repeat(40),
            },
            "old active".into(),
            10,
        );
        old_active.last_activity_at = 10;
        let mut new_active = Review::new(
            repo.clone(),
            ChangesetKey::Single {
                sha: "2".repeat(40),
            },
            "new active".into(),
            20,
        );
        new_active.last_activity_at = 20;
        let mut done = Review::new(
            repo.clone(),
            ChangesetKey::Single {
                sha: "3".repeat(40),
            },
            "done".into(),
            30,
        );
        done.status = ReviewStatus::Completed;
        done.completed_at = Some(31);
        done.last_activity_at = 31;
        let other_repo = Review::new(
            dir.path().join("elsewhere"),
            ChangesetKey::Single {
                sha: "4".repeat(40),
            },
            "other".into(),
            40,
        );
        for review in [old_active, new_active, done, other_repo] {
            store.insert(review).expect("insert");
        }

        let names: Vec<&str> = store
            .for_repo(&repo)
            .iter()
            .map(|review| review.name.as_str())
            .collect();
        assert_eq!(names, vec!["new active", "old active", "done"]);
    }

    #[test]
    fn mutate_applies_in_memory_and_saves() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        let mut store = ReviewStore::load(Some(reviews_dir.clone()));
        let review = sample(&dir.path().join("repo"), "before", 1);
        let id = review.id.clone();
        store.insert(review).expect("insert");

        let known = store
            .mutate(&id, |review| review.name = "after".into())
            .expect("mutate");
        assert!(known);
        assert_eq!(store.get(&id).expect("get").name, "after");
        let reloaded = ReviewStore::load(Some(reviews_dir));
        assert_eq!(reloaded.get(&id).expect("reload").name, "after");
    }

    #[test]
    fn delete_removes_the_file_and_the_entry() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        let mut store = ReviewStore::load(Some(reviews_dir.clone()));
        let review = sample(&dir.path().join("repo"), "doomed", 1);
        let id = review.id.clone();
        store.insert(review).expect("insert");

        assert!(store.delete(&id).expect("delete"));
        assert_eq!(store.get(&id), None);
        assert!(!reviews_dir.join(format!("{id}.json")).is_file());
        assert!(!store.delete(&id).expect("second delete"));
    }

    #[test]
    fn store_without_a_dir_holds_state_in_memory_only() {
        let mut store = ReviewStore::load(None);
        let review = sample(Path::new("/repo"), "memory", 1);
        let id = review.id.clone();
        store.insert(review).expect("insert without dir");
        assert!(store.get(&id).is_some());
        assert!(store
            .mutate(&id, |review| review.name = "renamed".into())
            .expect("mutate"));
        assert!(store.delete(&id).expect("delete"));
    }

    #[test]
    fn review_guide_round_trips_through_the_store() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        let mut store = ReviewStore::load(Some(reviews_dir.clone()));
        let review = sample(&dir.path().join("repo"), "guided", 1);
        let id = review.id.clone();
        store.insert(review).expect("insert");

        let guide = ReviewGuide {
            summary: "Sessions now expire after inactivity.".to_string(),
            review_order: vec![ReviewGuideEntry {
                path: "src/session.rs".to_string(),
                note: "New expiry module; read first.".to_string(),
            }],
            generated_at: 1_750_000_000,
        };
        store
            .mutate(&id, |review| review.guide = Some(guide.clone()))
            .expect("mutate");

        let reloaded = ReviewStore::load(Some(reviews_dir));
        assert_eq!(reloaded.get(&id).expect("reload").guide, Some(guide));
    }

    #[test]
    fn reviews_without_a_guide_load_with_none() {
        // A review file written before the guide field existed must still load.
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        fs::create_dir_all(&reviews_dir).expect("mkdir");
        let json = format!(
            r#"{{"id":"g1","repo_path":"/r","changeset":{{"kind":"single","sha":"{}"}},
                "name":"n","created_at":1,"last_activity_at":2,"status":"active"}}"#,
            "c".repeat(40)
        );
        fs::write(reviews_dir.join("g1.json"), json).expect("write");

        let store = ReviewStore::load(Some(reviews_dir));
        assert_eq!(store.get("g1").expect("loads").guide, None);
    }

    #[test]
    fn review_threads_round_trip_through_the_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = ReviewStore::load(Some(dir.path().to_path_buf()));
        let review = Review::new(
            PathBuf::from("/repo"),
            ChangesetKey::Single { sha: "abc".into() },
            "Named".into(),
            7,
        );
        let id = review.id.clone();
        store.insert(review).expect("insert");
        store
            .mutate(&id, |r| {
                r.threads.push(ReviewThread {
                    id: "c1".into(),
                    path: "src/sales.py".into(),
                    kind: ReviewThreadKind::Note,
                    anchor: ThreadAnchor {
                        side: ThreadSide::New,
                        start_line: 62,
                        start_col: 4,
                        end_line: 63,
                        end_col: 10,
                        quoted_text: "line[\"revenue\"] = round(".into(),
                    },
                    messages: vec![ThreadMessage {
                        id: "m1".into(),
                        author: MessageAuthor::Reviewer,
                        body: "Rounding per entry can drift.".into(),
                        created_at: 99,
                    }],
                    body: String::new(),
                    created_at: 99,
                });
            })
            .expect("mutate");

        let reloaded = ReviewStore::load(Some(dir.path().to_path_buf()));
        let review = reloaded.get(&id).expect("review reloads");
        assert_eq!(review.threads.len(), 1);
        let thread = &review.threads[0];
        assert_eq!(thread.path, "src/sales.py");
        assert_eq!(thread.anchor.side, ThreadSide::New);
        assert_eq!(thread.anchor.start_line, 62);
        assert_eq!(thread.anchor.quoted_text, "line[\"revenue\"] = round(");
        assert_eq!(thread.messages.len(), 1);
        assert_eq!(thread.messages[0].body, "Rounding per entry can drift.");
    }

    #[test]
    fn reviews_without_threads_load_with_an_empty_list() {
        // A pre-threads review document must deserialize with threads = [].
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        fs::create_dir_all(&reviews_dir).expect("mkdir");
        let json = format!(
            r#"{{"id":"old-doc","repo_path":"/repo","changeset":{{"kind":"single","sha":"{}"}},
                "name":"Old","created_at":1,"last_activity_at":1,"status":"active"}}"#,
            "abc".repeat(14) + "ab"
        );
        fs::write(reviews_dir.join("old-doc.json"), json).expect("write");

        let store = ReviewStore::load(Some(reviews_dir));
        let review = store.get("old-doc").expect("loads");
        assert!(review.threads.is_empty());
    }

    #[test]
    fn legacy_single_body_threads_normalize_to_one_message() {
        let mut thread = ReviewThread {
            id: "t1".into(),
            path: "src/lib.rs".into(),
            kind: ReviewThreadKind::Note,
            anchor: ThreadAnchor::default(),
            messages: Vec::new(),
            body: "legacy body".into(),
            created_at: 42,
        };
        thread.normalize();
        assert_eq!(
            thread.messages.len(),
            1,
            "legacy body folds into one message"
        );
        let message = &thread.messages[0];
        assert_eq!(message.author, MessageAuthor::Reviewer);
        assert_eq!(message.body, "legacy body");
        assert_eq!(message.created_at, 42);
        assert!(
            !message.id.is_empty(),
            "normalize assigns the message an id"
        );
        assert_eq!(
            thread.body, "",
            "the legacy field is cleared once folded into messages"
        );
    }

    #[test]
    fn thread_kind_defaults_to_note_for_legacy_documents() {
        // A thread document written before the `kind` field existed must
        // deserialize with kind = Note (agent threads are a Task 4 addition).
        let json = r#"{"id":"t1","path":"src/lib.rs",
            "anchor":{"side":"new","start_line":1,"start_col":0,"end_line":1,
            "end_col":1,"quoted_text":"x"},
            "messages":[{"id":"m1","author":"reviewer","body":"note","created_at":1}],
            "created_at":1}"#;
        let thread: ReviewThread = serde_json::from_str(json).expect("legacy thread loads");
        assert_eq!(
            thread.kind,
            ReviewThreadKind::Note,
            "a thread with no kind field defaults to Note"
        );
    }

    #[test]
    fn thread_kind_round_trips_through_serde() {
        let thread = ReviewThread {
            id: "t1".into(),
            path: "src/lib.rs".into(),
            kind: ReviewThreadKind::Agent,
            anchor: ThreadAnchor::default(),
            messages: vec![ThreadMessage {
                id: "m1".into(),
                author: MessageAuthor::Reviewer,
                body: "why?".into(),
                created_at: 1,
            }],
            body: String::new(),
            created_at: 1,
        };
        let json = serde_json::to_string(&thread).expect("serialize");
        assert!(
            json.contains("\"kind\":\"agent\""),
            "the agent kind serializes as snake_case: {json}"
        );
        let reloaded: ReviewThread = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(reloaded.kind, ReviewThreadKind::Agent, "kind round-trips");
    }

    #[test]
    fn threads_with_messages_round_trip() {
        let thread = ReviewThread {
            id: "t2".into(),
            path: "src/lib.rs".into(),
            kind: ReviewThreadKind::Note,
            anchor: ThreadAnchor::default(),
            messages: vec![
                ThreadMessage {
                    id: "m1".into(),
                    author: MessageAuthor::Reviewer,
                    body: "first".into(),
                    created_at: 10,
                },
                ThreadMessage {
                    id: "m2".into(),
                    author: MessageAuthor::Agent,
                    body: "second".into(),
                    created_at: 20,
                },
            ],
            body: String::new(),
            created_at: 10,
        };

        let json = serde_json::to_string(&thread).expect("serialize");
        let mut reloaded: ReviewThread = serde_json::from_str(&json).expect("deserialize");
        reloaded.normalize();

        assert_eq!(
            reloaded, thread,
            "new-format threads round trip unchanged and normalize is a no-op"
        );
    }

    #[test]
    fn last_activity_is_the_latest_message() {
        let thread = ReviewThread {
            id: "t3".into(),
            path: "src/lib.rs".into(),
            kind: ReviewThreadKind::Note,
            anchor: ThreadAnchor::default(),
            messages: vec![
                ThreadMessage {
                    id: "m1".into(),
                    author: MessageAuthor::Reviewer,
                    body: "first".into(),
                    created_at: 10,
                },
                ThreadMessage {
                    id: "m2".into(),
                    author: MessageAuthor::Reviewer,
                    body: "reply".into(),
                    created_at: 50,
                },
            ],
            body: String::new(),
            created_at: 10,
        };
        assert_eq!(thread.last_activity_at(), 50);

        let empty = ReviewThread {
            id: "t4".into(),
            path: "src/lib.rs".into(),
            kind: ReviewThreadKind::Note,
            anchor: ThreadAnchor::default(),
            messages: Vec::new(),
            body: String::new(),
            created_at: 99,
        };
        assert_eq!(
            empty.last_activity_at(),
            99,
            "with no messages, activity falls back to created_at"
        );
    }

    #[test]
    fn legacy_comments_key_loads_into_threads_and_round_trips_as_threads() {
        // A review document persisted before the threads rename used the key
        // "comments", and each thread was a single-body document (before
        // messages existed). It must still load — both legacy layers folded
        // — and the next save must re-emit "threads" with a "messages" array
        // (not "comments" or a top-level "body").
        let dir = tempfile::tempdir().expect("create tempdir");
        let reviews_dir = dir.path().join("reviews");
        fs::create_dir_all(&reviews_dir).expect("mkdir");
        let json = format!(
            r#"{{"id":"legacy-doc","repo_path":"/repo","changeset":{{"kind":"single","sha":"{}"}},
                "name":"Legacy","created_at":1,"last_activity_at":1,"status":"active",
                "comments":[{{"id":"c1","path":"src/lib.rs","anchor":{{"side":"new",
                "start_line":1,"start_col":0,"end_line":1,"end_col":1,"quoted_text":"x"}},
                "body":"legacy body","created_at":2}}]}}"#,
            "d".repeat(40)
        );
        fs::write(reviews_dir.join("legacy-doc.json"), json).expect("write");

        let mut store = ReviewStore::load(Some(reviews_dir.clone()));
        let review = store.get("legacy-doc").expect("loads");
        assert_eq!(review.threads.len(), 1);
        assert_eq!(review.threads[0].messages.len(), 1);
        assert_eq!(review.threads[0].messages[0].body, "legacy body");
        assert_eq!(
            review.threads[0].messages[0].author,
            MessageAuthor::Reviewer
        );

        // Trigger a save (no-op mutation) and confirm the on-disk shape is
        // fully migrated: "threads"/"messages", no "comments" key, and the
        // legacy top-level "body" is dropped now that it's empty (the
        // reviewer's text moved into the message).
        store
            .mutate("legacy-doc", |_| {})
            .expect("mutate triggers save");
        let raw = fs::read_to_string(reviews_dir.join("legacy-doc.json")).expect("read saved");
        assert!(raw.contains("\"threads\""));
        assert!(!raw.contains("\"comments\""));
        assert!(raw.contains("\"messages\""));
        assert!(raw.contains("\"legacy body\""));
    }
}
