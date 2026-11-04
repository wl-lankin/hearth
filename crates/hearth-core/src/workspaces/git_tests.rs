use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspaces::git::{self, MergeOutcome};

/// Runs git in `dir` for the test setup; panics on failure.
pub(crate) fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repo on `main` with one commit of `app.txt`.
pub(crate) fn repo(root: &Path) -> PathBuf {
    let dir = root.join("shop");
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    sh(&dir, &["config", "user.email", "t@example.com"]);
    sh(&dir, &["config", "user.name", "Test"]);
    sh(&dir, &["config", "core.autocrlf", "false"]);
    std::fs::write(dir.join("app.txt"), "one\n").unwrap();
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-q", "-m", "start"]);
    dir
}

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-q", "-m", file]);
}

#[test]
fn a_worktree_gets_a_new_branch_or_an_existing_one() {
    let d = tempfile::tempdir().unwrap();
    let repo = repo(d.path());
    let wt = d.path().join("ws").join("feature-x");
    git::add(&repo, &wt, "feature/x", "main").unwrap();
    assert_eq!(git::current_branch(&wt).as_deref(), Some("feature/x"));
    assert_eq!(std::fs::read_to_string(wt.join("app.txt")).unwrap(), "one\n");
    sh(&repo, &["branch", "old"]);
    let wt2 = d.path().join("ws").join("old");
    git::add(&repo, &wt2, "old", "main").unwrap();
    assert_eq!(git::current_branch(&wt2).as_deref(), Some("old"));
    assert_eq!(git::linked_worktree_parent(&wt).map(|p| p.canonicalize().unwrap()), Some(repo.canonicalize().unwrap()));
    assert_eq!(git::linked_worktree_parent(&repo), None, "the main checkout is no linked worktree");
    let err = git::add(&repo, &d.path().join("ws").join("again"), "feature/x", "main").unwrap_err().to_string();
    assert!(err.contains("feature/x"), "{err}");
}

#[test]
fn merge_fast_forwards_or_merges_and_never_leaves_a_conflict_behind() {
    let d = tempfile::tempdir().unwrap();
    let repo = repo(d.path());
    let wt = d.path().join("ws");
    git::add(&repo, &wt, "feature", "main").unwrap();
    commit(&wt, "a.txt", "a\n");
    assert_eq!(git::merge(&repo, "feature", "main").unwrap(), MergeOutcome::FastForward);
    assert!(repo.join("a.txt").is_file(), "the parent's checkout follows");
    commit(&wt, "b.txt", "b\n");
    commit(&repo, "c.txt", "c\n");
    assert_eq!(git::merge(&repo, "feature", "main").unwrap(), MergeOutcome::Merged);
    assert!(repo.join("b.txt").is_file() && repo.join("c.txt").is_file());
    commit(&wt, "app.txt", "workspace\n");
    commit(&repo, "app.txt", "parent\n");
    let err = git::merge(&repo, "feature", "main").unwrap_err().to_string();
    assert_eq!(err, "The merge has conflicts: merge `main` into the workspace and resolve them there");
    assert!(git::is_clean(&repo), "the merge was aborted");
    std::fs::write(repo.join("dirty.txt"), "x").unwrap();
    sh(&repo, &["add", "dirty.txt"]);
    commit(&wt, "d.txt", "d\n");
    let err = git::merge(&repo, "feature", "main").unwrap_err().to_string();
    assert!(err.contains("uncommitted changes"), "{err}");
}

#[test]
fn a_worktree_with_read_only_files_is_removed() {
    let d = tempfile::tempdir().unwrap();
    let repo = repo(d.path());
    let wt = d.path().join("ws");
    git::add(&repo, &wt, "feature", "main").unwrap();
    let file = wt.join("app.txt");
    let mut perms = std::fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&file, perms).unwrap();
    std::fs::create_dir_all(wt.join("vendor").join("x")).unwrap();
    std::fs::write(wt.join("vendor").join("x").join("y.php"), "<?php").unwrap();
    git::remove(&repo, &wt).unwrap();
    assert!(!wt.exists());
    assert!(!sh(&repo, &["worktree", "list"]).contains("ws"));
    assert!(sh(&repo, &["branch", "--list", "feature"]).contains("feature"), "the branch stays");
}
