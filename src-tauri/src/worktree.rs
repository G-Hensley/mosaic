// Git-worktree isolation: an isolated session gets its own worktree + branch, so
// parallel agents editing the same repo never clash files. Worktrees live OUTSIDE
// the repo (app-local data) so they never pollute the working tree or `git status`.
//
// Note: each worktree is its own git root, which is why an isolated session needs
// its own Claude MCP registration (Claude keys local-scope servers by git root).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone)]
pub struct Worktree {
    pub repo: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    /// Commit the branch started from — used to tell whether the agent did work.
    pub base: String,
}

/// A worktree a previous run of the app created, as the frontend remembered it.
///
/// Persisted per pane so a restored session can go back to the worktree it was
/// already working in. Only what is needed to identify and re-adopt it is kept;
/// everything else about a `Worktree` is re-derived on reattach.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Saved {
    pub repo: String,
    pub path: String,
    pub branch: String,
    /// Commit the branch started from, carried forward so `remove` can still
    /// tell whether the agent committed anything of its own.
    pub base: String,
}

/// Outcome of trying to re-adopt a `Saved` worktree.
///
/// The distinction that matters is `Missing` versus `Unusable`: only the former
/// means a fresh worktree can safely be cut. See `reattach`.
pub enum Reattach {
    /// The worktree is still on disk and registered — the session goes back into it.
    Reused(Worktree),
    /// The directory is gone, so there is nothing left to strand.
    Missing,
    /// It belongs to a different repository than the one now in play (the user
    /// switched projects), so it is not this session's to re-adopt — and it is
    /// still tracked by its own repo, so ignoring it strands nothing.
    Foreign,
    /// The directory is still on disk but git cannot use it as a worktree of this
    /// repo. Carries why, for the message the user gets.
    Unusable(String),
}

/// Outcome of attempting to remove a worktree.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// Worktree was clean and has been removed (branch deleted if no unique commits).
    Removed,
    /// Worktree has uncommitted changes; removal refused to avoid data loss.
    /// The worktree and branch are left intact on disk.
    RefusedDirty,
    /// Worktree directory no longer exists; considered already removed.
    AlreadyGone,
}

fn git_out(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_ok(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if a worktree has uncommitted changes (dirty working tree or untracked files).
/// Runs against the worktree itself; the parent repo is irrelevant here.
fn is_dirty(worktree_path: &Path) -> Result<bool, String> {
    let path_s = worktree_path.to_string_lossy().to_string();
    // --porcelain gives machine-readable output; empty = clean.
    let status = git_out(&["-C", &path_s, "status", "--porcelain"])?;
    Ok(!status.is_empty())
}

/// Toplevel of the git repo containing `dir`, if any.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let d = dir.to_string_lossy().to_string();
    git_out(&["-C", &d, "rev-parse", "--show-toplevel"])
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Where worktrees are kept — outside any repo, and outside the install dir.
fn base_dir() -> PathBuf {
    crate::app_data_dir().join("worktrees")
}

/// Internal helper: create a worktree using a specific base directory.
/// Used by tests to keep worktrees inside a temp directory.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn create_with_base_dir(
    repo: &Path,
    session_id: &str,
    base_dir: &Path,
) -> Result<Worktree, String> {
    let uid = uuid::Uuid::new_v4().simple().to_string();
    let name = format!("{session_id}-{}", &uid[..6]);
    let branch = format!("mosaic/{name}");
    let path = base_dir.join(&name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let repo_s = repo.to_string_lossy().to_string();
    let base = git_out(&["-C", &repo_s, "rev-parse", "HEAD"])?;
    let path_s = path.to_string_lossy().to_string();
    git_out(&["-C", &repo_s, "worktree", "add", "-b", &branch, &path_s])?;

    Ok(Worktree {
        repo: repo.to_path_buf(),
        path,
        branch,
        base,
    })
}

/// Create an isolated worktree + branch for a session. The branch carries a short
/// unique suffix so re-used session ids across app runs never collide.
pub fn create(repo: &Path, session_id: &str) -> Result<Worktree, String> {
    create_with_base_dir(repo, session_id, &base_dir())
}

/// Compare two paths for "same directory on disk".
///
/// String equality is not enough: the path git prints in `worktree list` and the
/// one the frontend persisted can differ in separator and (on Windows) case
/// while naming the same directory, and treating them as different is the
/// dangerous direction — it reads as "the old worktree is gone" and cuts a
/// second one. Canonicalize when both sides resolve; fall back to a normalized
/// textual compare when either does not.
fn same_path(a: &Path, b: &Path) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim().trim_start_matches("\\\\?\\").replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => norm(&a.to_string_lossy()) == norm(&b.to_string_lossy()),
        _ => norm(&a.to_string_lossy()) == norm(&b.to_string_lossy()),
    }
}

/// Re-adopt the worktree a session was already running in, instead of cutting a
/// second one and abandoning the first.
///
/// This exists because an abandoned worktree is not merely untidy: it can hold
/// an agent's uncommitted work, which is exactly why `remove` refuses to delete
/// a dirty one. A worktree with no session pointing at it is work nothing in the
/// app can lead the user back to. So a directory that is still on disk is never
/// written off — `Unusable` is reported instead of `Missing`, and the caller
/// refuses the spawn rather than stranding it.
///
/// Registration with `repo` is the authority for "still a worktree", not the
/// mere existence of the directory: a pruned or moved-away entry leaves a
/// directory git will not accept commands for.
pub fn reattach(repo: &Path, saved: &Saved) -> Reattach {
    if !same_path(Path::new(&saved.repo), repo) {
        return Reattach::Foreign;
    }
    let path = PathBuf::from(&saved.path);
    if !path.exists() {
        return Reattach::Missing;
    }

    let repo_s = repo.to_string_lossy().to_string();
    let listed = match git_out(&["-C", &repo_s, "worktree", "list", "--porcelain"]) {
        Ok(out) => out,
        Err(e) => {
            return Reattach::Unusable(format!("git could not list this repo's worktrees: {e}"))
        }
    };
    let registered = listed
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|listed| same_path(Path::new(listed.trim()), &path));
    if !registered {
        return Reattach::Unusable(
            "it is no longer registered as a worktree of this repository".to_string(),
        );
    }

    // Read the branch back from the worktree rather than trusting the saved
    // name: the agent may have switched branches during the last run, and
    // `remove` decides whether a branch is safe to delete from this value.
    let path_s = path.to_string_lossy().to_string();
    let branch = match git_out(&["-C", &path_s, "rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(b) if !b.is_empty() && b != "HEAD" => b,
        // Detached HEAD — keep the recorded name so the branch is still tracked.
        Ok(_) => saved.branch.clone(),
        Err(e) => return Reattach::Unusable(format!("git cannot run inside it: {e}")),
    };

    Reattach::Reused(Worktree {
        repo: repo.to_path_buf(),
        path,
        branch,
        base: saved.base.clone(),
    })
}

/// Remove the worktree. The branch is deleted ONLY if it has no commits of its
/// own — we never silently discard an agent's work.
///
/// Returns:
/// - `Ok(RemoveOutcome::Removed)` if the worktree was clean and removed.
/// - `Ok(RemoveOutcome::RefusedDirty)` if the worktree has uncommitted changes;
///   the worktree and branch are left intact.
/// - `Ok(RemoveOutcome::AlreadyGone)` if the worktree path no longer exists.
/// - `Err(String)` on unexpected git failures.
pub fn remove(wt: &Worktree) -> Result<RemoveOutcome, String> {
    let repo = wt.repo.to_string_lossy().to_string();
    let path = wt.path.to_string_lossy().to_string();

    // If the worktree directory is already gone, treat as success (idempotent).
    if !std::path::Path::new(&path).exists() {
        // Still try to clean up the branch if it has no unique commits.
        let range = format!("{}..{}", wt.base, wt.branch);
        let unique =
            git_out(&["-C", &repo, "rev-list", "--count", &range]).unwrap_or_else(|_| "1".into());
        if unique.trim() == "0" {
            let _ = git_ok(&["-C", &repo, "branch", "-D", &wt.branch]);
        }
        let _ = git_ok(&["-C", &repo, "worktree", "prune"]);
        return Ok(RemoveOutcome::AlreadyGone);
    }

    // Check for uncommitted changes before doing anything destructive.
    if is_dirty(&wt.path)? {
        return Ok(RemoveOutcome::RefusedDirty);
    }

    // Clean: remove the worktree (force is safe now that we know it's clean).
    // Surface git's own stderr rather than reporting a removal that never happened.
    git_out(&["-C", &repo, "worktree", "remove", "--force", &path])?;

    // Delete the branch only if it has no commits of its own.
    let range = format!("{}..{}", wt.base, wt.branch);
    let unique =
        git_out(&["-C", &repo, "rev-list", "--count", &range]).unwrap_or_else(|_| "1".into());
    if unique.trim() == "0" {
        let _ = git_ok(&["-C", &repo, "branch", "-D", &wt.branch]);
    }
    let _ = git_ok(&["-C", &repo, "worktree", "prune"]);

    Ok(RemoveOutcome::Removed)
}

/// Git fixture shared by this module's tests and the spawn-rollback tests in
/// `lib.rs`, which need a real worktree to assert cleanup against.
#[cfg(test)]
pub(crate) fn init_test_repo(dir: &Path) -> Result<(), String> {
    tests::init_repo(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    pub(super) fn init_repo(dir: &Path) -> Result<(), String> {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .map_err(|e| e.to_string())?;
        Command::new("git")
            .args(["config", "user.email", "test@test.test"])
            .current_dir(dir)
            .output()
            .map_err(|e| e.to_string())?;
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .map_err(|e| e.to_string())?;
        fs::write(dir.join("README.md"), "# Test\n").map_err(|e| e.to_string())?;
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .map_err(|e| e.to_string())?;
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir)
            .output()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn git_commit_all(dir: &Path, msg: &str) -> Result<(), String> {
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .map_err(|e| e.to_string())?;
        Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(dir)
            .output()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Helper to create a test worktree inside the temp directory.
    fn create_test_wt(repo: &Path, session_id: &str, tmp: &TempDir) -> Worktree {
        create_with_base_dir(repo, session_id, tmp.path()).unwrap()
    }

    #[test]
    fn remove_clean_worktree_succeeds() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // Worktree is clean (no changes made)
        let result = remove(&wt).unwrap();
        assert_eq!(result, RemoveOutcome::Removed);
        // Worktree directory should be gone
        assert!(!wt.path.exists());
        // Branch should be deleted (no unique commits)
        let branches =
            git_out(&["-C", repo.to_str().unwrap(), "branch", "--list", &wt.branch]).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn remove_dirty_worktree_refused() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // Make an uncommitted change in the worktree
        fs::write(wt.path.join("dirty.txt"), "uncommitted").unwrap();

        let result = remove(&wt).unwrap();
        assert_eq!(result, RemoveOutcome::RefusedDirty);
        // Worktree and file should still exist
        assert!(wt.path.exists());
        assert!(wt.path.join("dirty.txt").exists());
        // Branch should still exist
        let branches =
            git_out(&["-C", repo.to_str().unwrap(), "branch", "--list", &wt.branch]).unwrap();
        assert!(branches.contains(&wt.branch));
    }

    #[test]
    fn remove_worktree_with_unique_commits_keeps_branch() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // Make a commit in the worktree
        fs::write(wt.path.join("new_file.txt"), "committed").unwrap();
        git_commit_all(&wt.path, "agent work").unwrap();

        let result = remove(&wt).unwrap();
        assert_eq!(result, RemoveOutcome::Removed);
        // Worktree directory should be gone
        assert!(!wt.path.exists());
        // Branch should be KEPT because it has unique commits
        let branches =
            git_out(&["-C", repo.to_str().unwrap(), "branch", "--list", &wt.branch]).unwrap();
        assert!(branches.contains(&wt.branch));
    }

    #[test]
    fn remove_already_gone_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // Manually remove the worktree directory
        fs::remove_dir_all(&wt.path).unwrap();

        // remove() should succeed and not error
        let result = remove(&wt).unwrap();
        assert_eq!(result, RemoveOutcome::AlreadyGone);
    }

    /// What the frontend persists for an isolated pane.
    fn saved_from(wt: &Worktree) -> Saved {
        Saved {
            repo: wt.repo.to_string_lossy().to_string(),
            path: wt.path.to_string_lossy().to_string(),
            branch: wt.branch.clone(),
            base: wt.base.clone(),
        }
    }

    #[test]
    fn reattach_reuses_a_worktree_that_is_still_on_disk() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-restore", &tmp);
        let saved = saved_from(&wt);

        match reattach(&repo, &saved) {
            Reattach::Reused(w) => {
                assert_eq!(w.path, wt.path, "must go back to the same directory");
                assert_eq!(w.branch, wt.branch);
                assert_eq!(w.base, wt.base, "base must survive so remove() still works");
            }
            _ => panic!("an intact worktree must be reused"),
        }
    }

    /// The trap this whole path exists to avoid: a restored session must land
    /// back on the uncommitted work, not beside it in a second worktree.
    #[test]
    fn reattach_keeps_uncommitted_work_reachable() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-dirty", &tmp);
        fs::write(wt.path.join("in-progress.txt"), "unsaved").unwrap();

        match reattach(&repo, &saved_from(&wt)) {
            Reattach::Reused(w) => {
                assert!(w.path.join("in-progress.txt").exists());
            }
            _ => panic!("a dirty worktree must be reused, never abandoned"),
        }
    }

    #[test]
    fn reattach_reports_missing_when_the_directory_is_gone() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-gone", &tmp);
        let saved = saved_from(&wt);
        // The ordinary case: the session exited clean last run and cleanup took it.
        assert_eq!(remove(&wt).unwrap(), RemoveOutcome::Removed);

        assert!(
            matches!(reattach(&repo, &saved), Reattach::Missing),
            "a removed worktree leaves nothing to strand, so a fresh one is fine"
        );
    }

    #[test]
    fn reattach_reports_unusable_when_the_directory_is_no_longer_a_worktree() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        // A directory that exists but git has no worktree registration for.
        let orphan = tmp.path().join("orphan");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("work.txt"), "might be the agent's").unwrap();
        let saved = Saved {
            repo: repo.to_string_lossy().to_string(),
            path: orphan.to_string_lossy().to_string(),
            branch: "mosaic/sess-orphan".to_string(),
            base: "HEAD".to_string(),
        };

        assert!(
            matches!(reattach(&repo, &saved), Reattach::Unusable(_)),
            "a directory still on disk must never be written off as gone"
        );
    }

    #[test]
    fn reattach_reports_foreign_when_the_project_changed() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let other = tmp.path().join("other");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&other).unwrap();
        init_repo(&repo).unwrap();
        init_repo(&other).unwrap();

        let wt = create_test_wt(&repo, "sess-moved", &tmp);

        assert!(
            matches!(reattach(&other, &saved_from(&wt)), Reattach::Foreign),
            "another repo's worktree is not this session's to adopt"
        );
    }

    #[test]
    fn is_dirty_detects_untracked_files() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // Create an untracked file (not added to git)
        fs::write(wt.path.join("untracked.txt"), "untracked").unwrap();

        assert!(is_dirty(&wt.path).unwrap());
    }

    #[test]
    fn is_dirty_detects_modified_files() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // Modify a tracked file
        fs::write(wt.path.join("README.md"), "# Modified\n").unwrap();

        assert!(is_dirty(&wt.path).unwrap());
    }

    #[test]
    fn is_dirty_clean_worktree_returns_false() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // No changes
        assert!(!is_dirty(&wt.path).unwrap());
    }
}
