//! git, run as a program: worktrees, merges and the questions a workspace asks its repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::CoreError;

/// Runs `git -C dir args`; the trimmed stdout, or an error with git's own message.
pub fn run(dir: &Path, args: &[&str]) -> Result<String, CoreError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        // Never wait for a password prompt nobody sees.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| CoreError::Php(format!("git could not be started ({e}). Install git and try again.")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let err = if err.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_string() } else { err };
    Err(CoreError::Php(format!("git {}: {err}", args.first().copied().unwrap_or_default())))
}

fn ok(dir: &Path, args: &[&str]) -> bool {
    run(dir, args).is_ok()
}

/// The checkout's root folder, if `dir` is inside one.
pub fn toplevel(dir: &Path) -> Option<PathBuf> {
    run(dir, &["rev-parse", "--show-toplevel"]).ok().map(PathBuf::from)
}

/// Whether the repository has a commit to branch from.
pub fn has_commits(dir: &Path) -> bool {
    ok(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])
}

/// The branch checked out in `dir`; `None` on a detached HEAD.
pub fn current_branch(dir: &Path) -> Option<String> {
    run(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).ok().filter(|b| !b.is_empty())
}

pub fn head(dir: &Path) -> Option<String> {
    run(dir, &["rev-parse", "HEAD"]).ok()
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    ok(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
}

/// No changes, staged or not (untracked files count too).
pub fn is_clean(dir: &Path) -> bool {
    run(dir, &["status", "--porcelain"]).is_ok_and(|s| s.is_empty())
}

/// Checks `branch` out in a new worktree at `path`: an existing branch as it is, a new one from `base`.
pub fn add(repo: &Path, path: &Path, branch: &str, base: &str) -> Result<(), CoreError> {
    let target = path.to_string_lossy();
    if branch_exists(repo, branch) {
        run(repo, &["worktree", "add", &target, branch])?;
    } else {
        run(repo, &["worktree", "add", "-b", branch, &target, base])?;
    }
    Ok(())
}

/// The main checkout of a linked worktree (`.git` is a file pointing into `<repo>/.git/worktrees`);
/// `None` for a main checkout or no checkout.
pub fn linked_worktree_parent(dir: &Path) -> Option<PathBuf> {
    let top = toplevel(dir)?;
    if !top.join(".git").is_file() {
        return None;
    }
    let common = run(&top, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok()?;
    let common = PathBuf::from(common);
    (common.file_name()? == ".git").then(|| common.parent().map(Path::to_path_buf)).flatten()
}

/// Removes the worktree at `path` and forgets it; the branch stays. When git cannot (read-only
/// files on Windows), `delete_folder` allows deleting the folder by hand.
pub fn remove(repo: &Path, path: &Path, delete_folder: bool) -> Result<(), CoreError> {
    let target = path.to_string_lossy();
    let removed = run(repo, &["worktree", "remove", "--force", &target]);
    if removed.is_err() && path.exists() {
        if !delete_folder {
            return removed.map(|_| ());
        }
        clear_read_only(path);
        std::fs::remove_dir_all(path).map_err(|e| CoreError::io(path, e))?;
    }
    run(repo, &["worktree", "prune"]).map(|_| ())
}

fn clear_read_only(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            clear_read_only(&path);
        } else if meta.permissions().readonly() {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    FastForward,
    Merged,
}

/// Brings `branch` into `base` in the parent checkout: a fast-forward when possible, else a merge
/// commit. A conflict is aborted, so the parent never stays mid-merge.
pub fn merge(parent: &Path, branch: &str, base: &str) -> Result<MergeOutcome, CoreError> {
    let checked_out = current_branch(parent);
    // Not checked out in the parent: a local push moves the branch, if it is a fast-forward.
    if checked_out.as_deref() != Some(base) && ok(parent, &["push", ".", &format!("{branch}:{base}")]) {
        return Ok(MergeOutcome::FastForward);
    }
    if checked_out.as_deref() != Some(base) {
        return Err(CoreError::Php(format!(
            "`{base}` moved on and the parent has `{}` checked out: merge `{base}` into the workspace first",
            checked_out.unwrap_or_else(|| "a detached HEAD".into())
        )));
    }
    if !is_clean(parent) {
        return Err(CoreError::Php(
            "The parent checkout has uncommitted changes: commit or stash them, then merge again".into(),
        ));
    }
    let before = head(parent);
    if run(parent, &["merge", "--no-edit", branch]).is_err() {
        let _ = run(parent, &["merge", "--abort"]);
        return Err(CoreError::Php(format!(
            "The merge has conflicts: merge `{base}` into the workspace and resolve them there"
        )));
    }
    // A fast-forward leaves no merge commit: the new HEAD is the branch itself.
    let fast_forward = head(parent) == run(parent, &["rev-parse", branch]).ok() && before != head(parent);
    Ok(if fast_forward { MergeOutcome::FastForward } else { MergeOutcome::Merged })
}

/// Pushes `base` to origin.
pub fn push_origin(parent: &Path, base: &str) -> Result<(), CoreError> {
    run(parent, &["push", "origin", base]).map(|_| ())
}
