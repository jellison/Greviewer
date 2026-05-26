# Commit Graph Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show real commit history after a repository opens, with each commit exposing the metadata required by the review workflow spec.

**Architecture:** Extend the existing repository snapshot with a bounded commit list read by `git2` at open time. Render that snapshot directly in the root app's repository-open mode as a first graph-mode history list. Keep lane layout, merge connectors, selection, and progressive loading out of this slice.

**Tech Stack:** Rust 2021, git2 `0.20`, gpui `0.2`, gpui-component `0.5`, `#[gpui::test]`, Cargo integration tests.

---

## File Structure

- Modify `src/repo/mod.rs`: add `CommitInfo`, bounded revwalk, date formatting, and unit tests.
- Modify `src/app.rs`: render the commit history list and keep the empty-repository state visible.
- Modify `tests/repo.rs`: assert fixture commit history.
- Modify `tests/smoke.rs`: assert action-driven repository opening exposes commit history.
- Use existing `tests/common/mod.rs`: no helper changes required.

### Task 1: Repository Commit Snapshots

**Files:**
- Modify: `src/repo/mod.rs`
- Modify: `tests/repo.rs`

- [x] **Step 1: Write failing repository tests**

Add assertions in `tests/repo.rs`:

```rust
#[test]
fn open_at_reads_the_two_commits_fixture() {
    let dir = common::load_fixture("two-commits");

    let snapshot = open_at(dir.path()).expect("open succeeds");
    let head = snapshot.head.expect("head present");

    assert_eq!(head.short_sha.len(), 7);
    assert_eq!(head.summary, "Update hello.txt");
    assert_eq!(snapshot.commits.len(), 2);
    assert_eq!(snapshot.commits[0].summary, "Update hello.txt");
    assert_eq!(snapshot.commits[1].summary, "Add hello.txt");
    assert!(snapshot.commits[0].is_head);
    assert!(!snapshot.commits[1].is_head);
    assert_eq!(snapshot.commits[0].short_sha.len(), 7);
    assert!(!snapshot.commits[0].sha.is_empty());
    assert!(!snapshot.commits[0].author.is_empty());
    assert!(!snapshot.commits[0].authored_date.is_empty());
}
```

Add unit coverage in `src/repo/mod.rs`:

```rust
#[test]
fn open_at_returns_commits_newest_first() {
    let (dir, oid_hex) = init_repo_with_one_commit();

    let snapshot = open_at(dir.path()).expect("open succeeds");

    assert_eq!(snapshot.commits.len(), 1);
    assert_eq!(snapshot.commits[0].sha, oid_hex);
    assert_eq!(snapshot.commits[0].summary, "Add hello.txt");
    assert!(snapshot.commits[0].is_head);
}

#[test]
fn open_at_returns_empty_commits_for_an_unborn_repo() {
    let dir = init_unborn_repo();

    let snapshot = open_at(dir.path()).expect("open succeeds");

    assert!(snapshot.head.is_none());
    assert!(snapshot.commits.is_empty());
}

#[test]
fn civil_date_formatting_handles_unix_epoch() {
    let date = format_authored_date(git2::Time::new(0, 0));

    assert_eq!(date, "1970-01-01");
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test repo
```

Expected: FAIL to compile because `OpenRepository::commits`, `CommitInfo`, and `format_authored_date` do not exist yet.

- [x] **Step 3: Implement commit snapshots**

In `src/repo/mod.rs`, add:

```rust
pub const INITIAL_COMMIT_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub summary: String,
    pub author: String,
    pub authored_timestamp: i64,
    pub authored_date: String,
    pub parent_count: usize,
    pub is_head: bool,
}
```

Update `OpenRepository`:

```rust
pub struct OpenRepository {
    pub path: PathBuf,
    pub head: Option<HeadInfo>,
    pub commits: Vec<CommitInfo>,
}
```

Read the head commit once, then build the commit list with a bounded revwalk ordered newest-to-oldest.

- [x] **Step 4: Run targeted tests**

Run:

```bash
cargo test repo
```

Expected: PASS.

### Task 2: Render Graph-Mode Commit Rows

**Files:**
- Modify: `src/app.rs`
- Modify: `tests/smoke.rs`

- [x] **Step 1: Write failing app-level test assertions**

In `tests/smoke.rs`, extend `boots_open_repo_renders_head_info`:

```rust
assert_eq!(repo.commits.len(), 2);
assert_eq!(repo.commits[0].summary, "Update hello.txt");
assert_eq!(repo.commits[1].summary, "Add hello.txt");
assert!(repo.commits[0].is_head);
```

- [x] **Step 2: Run the smoke test to verify it fails**

Run:

```bash
cargo test --test smoke
```

Expected: FAIL to compile until Task 1 is implemented, then fail until the app exposes and preserves commit history through the action-driven open path.

- [x] **Step 3: Render commit rows**

In `src/app.rs`, replace the repository-open centered placeholder with a full-window column:

```rust
Mode::RepoOpen { repo } => self.render_repo_open(repo)
```

Add a helper that renders the repository path, a HEAD summary or empty graph message, and one row per commit. Each row includes the HEAD marker, short SHA, summary, author, and authored date.

- [x] **Step 4: Run targeted app tests**

Run:

```bash
cargo test --test smoke
cargo test app::tests
```

Expected: PASS.

### Task 3: Full Verification

**Files:**
- All modified files.

- [x] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: PASS with no output that indicates formatting failure.

- [x] **Step 2: Run project verification**

Run:

```bash
bin/check
```

Expected: PASS.
