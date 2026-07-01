//! Shared test fixtures for the inline test modules of `app` and its
//! submodules: synthetic Git-repo builders, graph/commit constructors, the
//! app-window bootstrap, and small assertion helpers. `#[cfg(test)]`-only.
//! See ADR-0003 (testing strategy).

use super::*;
use crate::graph;
use crate::repo::{self, ChangeKind};
use crate::settings::{self, RecentRepository, Settings};
use git2::{IndexAddOption, Repository, Signature};
use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use std::{collections::BTreeSet, fs, path::PathBuf};

/// Open a window holding a freshly constructed `App`, with the
/// gpui-component theme installed. The theme global is required by themed
/// widgets such as the changeset resizable split.
pub(crate) fn add_app_window(cx: &mut TestAppContext) -> WindowHandle<App> {
    cx.update(gpui_component::init);
    cx.add_window(App::new)
}

/// Open an app window whose settings persist to `store_path`, so close/quit
/// persistence can be observed by reading the file back.
pub(crate) fn add_app_window_with_store_path(
    cx: &mut TestAppContext,
    store_path: PathBuf,
) -> WindowHandle<App> {
    cx.update(gpui_component::init);
    cx.add_window(move |window, cx| {
        App::new_with_settings_store_path(window, cx, store_path.clone())
    })
}

/// Write a settings file at `path` whose only populated field is the recent
/// repository list. Mirrors how the storage tests seed state on disk.
pub(crate) fn seed_recent_repositories(
    path: &std::path::Path,
    recent_repositories: Vec<RecentRepository>,
) {
    settings::save(
        path,
        &Settings {
            recent_repositories,
            window_state: None,
        },
    )
    .expect("seed settings store");
}

/// Read back just the recent repository list from the settings file.
pub(crate) fn load_recent_repositories(path: &std::path::Path) -> Vec<RecentRepository> {
    settings::load(path).recent_repositories
}

pub(crate) fn init_repo_with_one_commit() -> (tempfile::TempDir, String) {
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

pub(crate) fn graph_commit(sha: &str, parent_shas: &[&str]) -> graph::GraphCommit {
    graph::GraphCommit {
        sha: sha.to_string(),
        authored_timestamp: 0,
        parent_shas: parent_shas.iter().map(|sha| sha.to_string()).collect(),
    }
}

pub(crate) fn commit_info(sha: &str, parent_shas: &[&str]) -> repo::CommitInfo {
    repo::CommitInfo {
        sha: sha.to_string(),
        short_sha: sha.chars().take(7).collect(),
        summary: format!("commit {sha}"),
        author: "Tester".to_string(),
        authored_timestamp: 0,
        authored_date: "2026-01-01".to_string(),
        parent_shas: parent_shas.iter().map(|sha| sha.to_string()).collect(),
        branch_labels: Vec::new(),
        parent_count: parent_shas.len(),
        is_head: false,
    }
}

pub(crate) fn local_branch(name: &str, tip_sha: &str) -> repo::Branch {
    repo::Branch {
        name: name.to_string(),
        tip_sha: tip_sha.to_string(),
        is_head: false,
        kind: repo::BranchKind::Local,
    }
}

pub(crate) fn remote_branch(remote: &str, name: &str, tip_sha: &str) -> repo::Branch {
    repo::Branch {
        name: format!("{remote}/{name}"),
        tip_sha: tip_sha.to_string(),
        is_head: false,
        kind: repo::BranchKind::Remote {
            remote: remote.to_string(),
        },
    }
}

pub(crate) fn hidden(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

pub(crate) fn head_branch(name: &str, tip_sha: &str) -> repo::Branch {
    repo::Branch {
        name: name.to_string(),
        tip_sha: tip_sha.to_string(),
        is_head: true,
        kind: repo::BranchKind::Local,
    }
}

/// Extract (path, visibility) for every folder row.
pub(crate) fn folder_visibilities(rows: &[BranchTreeRow]) -> Vec<(String, FolderVisibility)> {
    rows.iter()
        .filter_map(|row| match row {
            BranchTreeRow::Folder(folder) => Some((folder.path.clone(), folder.visibility)),
            _ => None,
        })
        .collect()
}
pub(crate) fn init_repo_with_two_commits() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
    let root_oid = commit_all(&repo, "Add hello.txt", &[]);

    fs::write(dir.path().join("hello.txt"), "hello world\n").expect("update file");
    let update_oid = commit_all(&repo, "Update hello.txt", &[root_oid]);

    drop(repo);

    (dir, update_oid.to_string())
}

pub(crate) fn init_repo_with_python_change() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("cli.py"), "threshold = 10\n").expect("write file");
    let root_oid = commit_all(&repo, "Add cli.py", &[]);

    fs::write(dir.path().join("cli.py"), "threshold = None\n").expect("update file");
    let update_oid = commit_all(&repo, "Update cli.py", &[root_oid]);

    drop(repo);
    (dir, update_oid.to_string())
}

/// Two commits on master (HEAD at the tip) plus a `feature` branch pointing
/// at the root commit. Returns (dir, master_tip_sha, root_sha).
pub(crate) fn init_repo_with_feature_branch() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
    let root_oid = commit_all(&repo, "Root", &[]);

    fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
    let main_tip = commit_all(&repo, "Main tip", &[root_oid]);

    let root_commit = repo.find_commit(root_oid).expect("find root commit");
    repo.branch("feature", &root_commit, false)
        .expect("create feature branch");

    drop(root_commit);
    drop(repo);
    (dir, main_tip.to_string(), root_oid.to_string())
}

/// Two commits on master plus a `feature` branch carrying one exclusive
/// commit branched from the root. Returns (dir, master_tip_sha,
/// feature_tip_sha). HEAD stays on master.
/// Timestamps are fixed so the loaded order is deterministic: main tip, feature tip, root.
pub(crate) fn init_repo_with_unmerged_branch_commit() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
    let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

    fs::write(dir.path().join("feature.txt"), "feature\n").expect("write feature file");
    let feature_tip = commit_all_to_ref_at_time(
        &repo,
        Some("refs/heads/feature"),
        "Feature work",
        &[root_oid],
        20,
    );

    fs::remove_file(dir.path().join("feature.txt")).expect("remove feature file");
    fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
    let main_tip = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Main tip", &[root_oid], 30);

    drop(repo);
    (dir, main_tip.to_string(), feature_tip.to_string())
}

/// Two commits on master (HEAD at the tip) plus `features/alpha` carrying
/// one exclusive commit and `features/beta` pointing at the root.
/// Exercises sidebar folder nesting. Timestamps are fixed so the loaded
/// order is deterministic. Returns (dir, master_tip_sha, alpha_tip_sha).
pub(crate) fn init_repo_with_slash_named_branches() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
    let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

    fs::write(dir.path().join("alpha.txt"), "alpha\n").expect("write alpha file");
    let alpha_tip = commit_all_to_ref_at_time(
        &repo,
        Some("refs/heads/features/alpha"),
        "Alpha work",
        &[root_oid],
        20,
    );

    let root_commit = repo.find_commit(root_oid).expect("find root commit");
    repo.branch("features/beta", &root_commit, false)
        .expect("create features/beta");
    drop(root_commit);

    fs::remove_file(dir.path().join("alpha.txt")).expect("remove alpha file");
    fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
    let main_tip = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Main tip", &[root_oid], 30);

    drop(repo);
    (dir, main_tip.to_string(), alpha_tip.to_string())
}

/// A repo whose checked-out branch has ~200 sibling local branches named
/// `branch-000` .. `branch-199`. Zero-padded so the sidebar's alphabetical
/// row order is deterministic, which lets virtualization tests assert that an
/// early row is materialized and a far one is culled.
pub(crate) fn init_repo_with_many_branches() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
    let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);
    let root_commit = repo.find_commit(root_oid).expect("find root commit");

    for index in 0..200 {
        let name = format!("branch-{index:03}");
        repo.branch(&name, &root_commit, false)
            .unwrap_or_else(|err| panic!("create branch {name}: {err}"));
    }

    drop(root_commit);
    drop(repo);
    dir
}

/// A repo with a checked-out master, a remote-tracking origin/master at the
/// same tip, and a remote-only origin/feature/x one commit ahead.
pub(crate) fn init_repo_with_remote_branches() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
    let root = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 100);

    fs::write(dir.path().join("feature.txt"), "feature\n").expect("write feature file");
    let remote_tip = commit_all_to_ref_at_time(&repo, None, "Remote feature", &[root], 200);

    repo.remote("origin", "https://example.invalid/repo.git")
        .expect("configure remote");
    repo.reference("refs/remotes/origin/master", root, true, "remote master")
        .expect("create remote master");
    repo.reference(
        "refs/remotes/origin/feature/x",
        remote_tip,
        true,
        "remote feature",
    )
    .expect("create remote feature");
    drop(repo);

    (dir, root.to_string(), remote_tip.to_string())
}

pub(crate) fn init_repo_with_detached_head() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("detached.txt"), "base\n").expect("write file");
    let root_oid = commit_all(&repo, "Base", &[]);

    fs::write(dir.path().join("detached.txt"), "tip\n").expect("update file");
    let tip_oid = commit_all(&repo, "Tip", &[root_oid]);
    repo.set_head_detached(tip_oid).expect("detach HEAD");

    drop(repo);

    (dir, tip_oid.to_string())
}

/// One commit with HEAD detached and the initial branch deleted, so the
/// repository has zero local branches.
pub(crate) fn init_repo_with_detached_head_no_branches() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("solo.txt"), "solo\n").expect("write file");
    let tip_oid = commit_all(&repo, "Solo", &[]);
    repo.set_head_detached(tip_oid).expect("detach HEAD");
    let mut branch = repo
        .find_branch("master", git2::BranchType::Local)
        .expect("find master");
    branch.delete().expect("delete master");

    drop(branch);
    drop(repo);
    (dir, tip_oid.to_string())
}

pub(crate) fn init_repo_with_changed_and_context_files() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("changed.txt"), "before\n").expect("write changed file");
    fs::write(dir.path().join("context.txt"), "context\n").expect("write context file");
    let root_oid = commit_all(&repo, "Initial", &[]);

    fs::write(dir.path().join("changed.txt"), "after\n").expect("update changed file");
    let update_oid = commit_all(&repo, "Update changed file", &[root_oid]);

    drop(repo);

    (dir, update_oid.to_string())
}

pub(crate) fn init_repo_with_nested_changed_and_context_files() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/changed.txt"), "before\n").expect("write changed file");
    fs::write(dir.path().join("src/context.txt"), "context\n").expect("write context file");
    let root_oid = commit_all(&repo, "Initial", &[]);

    fs::write(dir.path().join("src/changed.txt"), "after\n").expect("update changed file");
    let update_oid = commit_all(&repo, "Update changed file", &[root_oid]);

    drop(repo);

    (dir, update_oid.to_string())
}

pub(crate) fn init_repo_with_deeply_nested_long_paths() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    let deep_dir = "deeply/nested/directory/structure/that/keeps/going";
    let long_names = [
        "a_very_long_changed_file_name_for_horizontal_overflow_one.txt",
        "a_very_long_changed_file_name_for_horizontal_overflow_two.txt",
        "a_very_long_changed_file_name_for_horizontal_overflow_three.txt",
        "a_very_long_changed_file_name_for_horizontal_overflow_four.txt",
        "a_very_long_changed_file_name_for_horizontal_overflow_five.txt",
        "a_very_long_changed_file_name_for_horizontal_overflow_six.txt",
    ];
    fs::create_dir_all(dir.path().join(deep_dir)).expect("create nested dirs");
    for name in long_names {
        fs::write(dir.path().join(deep_dir).join(name), "before\n").expect("write file");
    }
    let root_oid = commit_all(&repo, "Initial", &[]);

    for name in long_names {
        fs::write(dir.path().join(deep_dir).join(name), "after\n").expect("update file");
    }
    let update_oid = commit_all(&repo, "Update files", &[root_oid]);

    drop(repo);
    (dir, update_oid.to_string())
}

pub(crate) fn init_repo_with_nested_line_stat_changes() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/notes.txt"), "keep\nold\n").expect("write notes file");
    let root_oid = commit_all(&repo, "Initial", &[]);

    fs::write(dir.path().join("src/notes.txt"), "keep\nnew\nextra\n").expect("update notes file");
    let update_oid = commit_all(&repo, "Update notes file", &[root_oid]);

    drop(repo);

    (dir, update_oid.to_string())
}

pub(crate) fn init_repo_with_three_commits() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("range.txt"), "one\n").expect("write file");
    let first_oid = commit_all(&repo, "First", &[]);

    fs::write(dir.path().join("range.txt"), "two\n").expect("update file");
    let second_oid = commit_all(&repo, "Second", &[first_oid]);

    fs::write(dir.path().join("range.txt"), "three\n").expect("update file again");
    let third_oid = commit_all(&repo, "Third", &[second_oid]);

    drop(repo);

    (
        dir,
        vec![
            third_oid.to_string(),
            second_oid.to_string(),
            first_oid.to_string(),
        ],
    )
}

pub(crate) fn init_repo_with_empty_rollup_range() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("roundtrip.txt"), "base\n").expect("write base file");
    let base_oid = commit_all(&repo, "Base", &[]);

    fs::write(dir.path().join("roundtrip.txt"), "changed\n").expect("change file");
    let change_oid = commit_all(&repo, "Change file", &[base_oid]);

    fs::write(dir.path().join("roundtrip.txt"), "base\n").expect("revert file");
    let revert_oid = commit_all(&repo, "Revert file", &[change_oid]);

    drop(repo);

    (
        dir,
        vec![
            revert_oid.to_string(),
            change_oid.to_string(),
            base_oid.to_string(),
        ],
    )
}

pub(crate) fn init_repo_with_linear_history(count: usize) -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");
    let mut newest_first = Vec::with_capacity(count);
    let mut parents = Vec::new();

    for index in 0..count {
        fs::write(dir.path().join("history.txt"), format!("commit {index}\n"))
            .expect("write history file");
        let oid = commit_all(&repo, &format!("Commit {index}"), &parents);
        newest_first.insert(0, oid.to_string());
        parents = vec![oid];
    }

    drop(repo);
    (dir, newest_first)
}

pub(crate) fn init_repo_with_diverged_history() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("base.txt"), "base\n").expect("write base file");
    let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

    fs::write(dir.path().join("left.txt"), "left\n").expect("write left file");
    let left_oid =
        commit_all_to_ref_at_time(&repo, Some("refs/heads/left"), "Left", &[root_oid], 30);

    fs::remove_file(dir.path().join("left.txt")).expect("remove left file");
    fs::write(dir.path().join("right.txt"), "right\n").expect("write right file");
    let right_oid =
        commit_all_to_ref_at_time(&repo, Some("refs/heads/right"), "Right", &[root_oid], 20);

    fs::write(dir.path().join("left.txt"), "left\n").expect("restore left file");
    let merge_oid = commit_all_to_ref_at_time(&repo, None, "Merge", &[left_oid, right_oid], 40);
    repo.reference("refs/heads/master", merge_oid, true, "update test HEAD")
        .expect("point HEAD branch at merge");

    drop(repo);

    (dir, left_oid.to_string(), right_oid.to_string())
}

pub(crate) fn commit_info_for_graph_at(
    sha: &str,
    authored_timestamp: i64,
    parents: &[&str],
) -> crate::repo::CommitInfo {
    crate::repo::CommitInfo {
        sha: sha.to_string(),
        short_sha: sha.chars().take(7).collect(),
        summary: sha.to_string(),
        author: "Greviewer Tests".to_string(),
        authored_timestamp,
        authored_date: "1970-01-01".to_string(),
        parent_shas: parents.iter().map(|parent| parent.to_string()).collect(),
        branch_labels: Vec::new(),
        parent_count: parents.len(),
        is_head: false,
    }
}

pub(crate) fn seed_repo_open_mode_with_commits(
    app: &mut App,
    path: PathBuf,
    commits: Vec<crate::repo::CommitInfo>,
) {
    let head = commits.first().map(|commit| crate::repo::HeadInfo {
        short_sha: commit.short_sha.clone(),
        summary: commit.summary.clone(),
    });

    app.mode = Mode::RepoOpen {
        repo: crate::repo::OpenRepository {
            path,
            head,
            commits,
            has_more_commits: false,
            branches: Vec::new(),
        },
    };
}

pub(crate) fn init_repo_with_merge_range() -> (tempfile::TempDir, String, String, String, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("base.txt"), "base\n").expect("write base file");
    let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

    fs::write(dir.path().join("side.txt"), "side\n").expect("write side file");
    let side_oid =
        commit_all_to_ref_at_time(&repo, Some("refs/heads/side"), "Side", &[root_oid], 20);

    fs::remove_file(dir.path().join("side.txt")).expect("remove side file");
    fs::write(dir.path().join("main.txt"), "main\n").expect("write main file");
    let main_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Main", &[root_oid], 30);

    fs::write(dir.path().join("side.txt"), "side\n").expect("restore side file");
    let merge_oid =
        commit_all_to_ref_at_time(&repo, Some("HEAD"), "Merge", &[main_oid, side_oid], 40);

    drop(repo);

    (
        dir,
        merge_oid.to_string(),
        main_oid.to_string(),
        side_oid.to_string(),
        root_oid.to_string(),
    )
}

pub(crate) fn init_repo_with_deleted_file() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("obsolete.txt"), "obsolete\n").expect("write obsolete file");
    let root_oid = commit_all(&repo, "Add obsolete.txt", &[]);

    fs::remove_file(dir.path().join("obsolete.txt")).expect("delete obsolete file");
    let delete_oid = commit_all(&repo, "Delete obsolete.txt", &[root_oid]);

    drop(repo);

    (dir, delete_oid.to_string())
}

pub(crate) fn init_repo_with_long_diff() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    let old_text = (1..=160)
        .map(|line| format!("old line {line:03}\n"))
        .collect::<String>();
    fs::write(dir.path().join("long.txt"), old_text).expect("write old file");
    let root_oid = commit_all(&repo, "Add long file", &[]);

    let new_text = (1..=160)
        .map(|line| format!("new line {line:03}\n"))
        .collect::<String>();
    fs::write(dir.path().join("long.txt"), new_text).expect("write new file");
    let update_oid = commit_all(&repo, "Update long file", &[root_oid]);

    drop(repo);

    (dir, update_oid.to_string())
}

pub(crate) fn init_repo_with_binary_file() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(dir.path().join("binary.dat"), b"\xff\xfe\0data").expect("write binary file");
    let oid = commit_all(&repo, "Add binary file", &[]);

    drop(repo);

    (dir, oid.to_string())
}

pub(crate) fn init_repo_with_renamed_file() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    fs::write(
        dir.path().join("old.txt"),
        "line one\nline two\nline three\nold line\nline five\n",
    )
    .expect("write old file");
    let root_oid = commit_all(&repo, "Add old.txt", &[]);

    fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).expect("rename file");
    fs::write(
        dir.path().join("new.txt"),
        "line one\nline two\nline three\nnew line\nline five\n",
    )
    .expect("update renamed file");
    let rename_oid = commit_all(&repo, "Rename old.txt", &[root_oid]);

    drop(repo);

    (dir, rename_oid.to_string())
}

pub(crate) fn test_debug_selector(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

pub(crate) fn commit_all(repo: &Repository, message: &str, parents: &[git2::Oid]) -> git2::Oid {
    commit_all_to_ref(repo, Some("HEAD"), message, parents)
}

pub(crate) fn commit_all_to_ref(
    repo: &Repository,
    update_ref: Option<&str>,
    message: &str,
    parents: &[git2::Oid],
) -> git2::Oid {
    let sig =
        Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
    commit_all_to_ref_with_signature(repo, update_ref, message, parents, &sig)
}

pub(crate) fn commit_all_to_ref_at_time(
    repo: &Repository,
    update_ref: Option<&str>,
    message: &str,
    parents: &[git2::Oid],
    seconds: i64,
) -> git2::Oid {
    let time = git2::Time::new(seconds, 0);
    let sig = Signature::new("Greviewer Tests", "tests@greviewer.invalid", &time)
        .expect("create signature");
    commit_all_to_ref_with_signature(repo, update_ref, message, parents, &sig)
}

pub(crate) fn commit_all_to_ref_with_signature(
    repo: &Repository,
    update_ref: Option<&str>,
    message: &str,
    parents: &[git2::Oid],
    sig: &Signature<'_>,
) -> git2::Oid {
    let mut index = repo.index().expect("open index");
    index.clear().expect("clear index");
    index
        .add_all(["*"], IndexAddOption::DEFAULT, None)
        .expect("stage files");
    index.write().expect("write index");
    let tree_oid = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_oid).expect("find tree");

    let parent_commits = parents
        .iter()
        .map(|oid| repo.find_commit(*oid).expect("find parent commit"))
        .collect::<Vec<_>>();
    let parent_refs = parent_commits.iter().collect::<Vec<_>>();

    repo.commit(update_ref, sig, sig, message, &tree, &parent_refs)
        .expect("create commit")
}
/// Open a deeply-nested-long-paths repo, select its single commit, open the
/// changeset view, and resize the window to 360×200. Returns the window
/// handle (for `read_with` access to app state) and the visual context (for
/// `debug_bounds` / simulate calls).
///
/// Both `file_tree_scrolls_both_axes` and `file_tree_rows_are_uniform_width`
/// share this setup; extract here to keep each test focused on its own
/// assertions.
pub(crate) fn open_deeply_nested_changeset_at_360x200(
    cx: &mut TestAppContext,
) -> (WindowHandle<App>, VisualTestContext) {
    use gpui::{px, size};

    let (dir, sha) = init_repo_with_deeply_nested_long_paths();
    let path = dir.path().to_path_buf();
    let window = add_app_window(cx);

    window
        .update(cx, |app, window, cx| {
            app.open_repository_at(path, window, cx);
            app.select_single_commit(sha.clone(), cx);
        })
        .expect("open repo and select commit");
    cx.run_until_parked();

    // Open the changeset view (transitions review_screen to Changeset mode,
    // which renders the file tree with changed-files-scroll).
    let mut visual = VisualTestContext::from_window(*window, cx);
    let open_bounds = visual
        .debug_bounds("open-changeset")
        .expect("open-changeset button must be visible after selecting a commit");
    visual.simulate_click(open_bounds.center(), Modifiers::none());
    cx.run_until_parked();

    visual.simulate_resize(size(px(360.), px(200.)));
    cx.run_until_parked();

    (window, visual)
}

pub(crate) fn changed_file_entry(path: &str) -> FileListEntry {
    FileListEntry::Changed(crate::repo::ChangedFile {
        path: path.to_string(),
        old_path: None,
        kind: ChangeKind::Modified,
        is_binary: false,
        line_stats: crate::repo::LineStats::default(),
    })
}

pub(crate) fn unchanged_file_entry(path: &str) -> FileListEntry {
    FileListEntry::Unchanged(crate::repo::RepositoryFile {
        path: path.to_string(),
    })
}

pub(crate) fn folder_collapsed(rows: &[FileTreeRow], folder_path: &str) -> bool {
    rows.iter()
        .find_map(|row| match row {
            FileTreeRow::Folder {
                path, collapsed, ..
            } if path == folder_path => Some(*collapsed),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing folder row for {folder_path}"))
}

/// Like `init_repo_with_slash_named_branches`, but HEAD is moved onto
/// `features/alpha`, so a sidebar folder contains the checked-out branch.
pub(crate) fn init_repo_with_head_inside_folder() -> (tempfile::TempDir, String) {
    let (dir, _master_tip, alpha_tip) = init_repo_with_slash_named_branches();
    let repo = Repository::open(dir.path()).expect("open repo");
    repo.set_head("refs/heads/features/alpha")
        .expect("set HEAD");
    drop(repo);
    (dir, alpha_tip)
}
