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

/// One persisted review. Future artifact collections (comments, AI threads,
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
                    let Ok(review) = serde_json::from_str::<Review>(&content) else {
                        continue;
                    };
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
}
