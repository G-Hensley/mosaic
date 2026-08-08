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
fn is_dirty(repo: &Path, worktree_path: &Path) -> Result<bool, String> {
    let _repo_s = repo.to_string_lossy().to_string();
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
fn create_with_base_dir(
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
    if is_dirty(&wt.repo, &wt.path)? {
        return Ok(RemoveOutcome::RefusedDirty);
    }

    // Clean: remove the worktree (force is safe now that we know it's clean).
    let _ = git_ok(&["-C", &repo, "worktree", "remove", "--force", &path]);

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_repo(dir: &Path) -> Result<(), String> {
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

    #[test]
    fn is_dirty_detects_untracked_files() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // Create an untracked file (not added to git)
        fs::write(wt.path.join("untracked.txt"), "untracked").unwrap();

        assert!(is_dirty(&wt.repo, &wt.path).unwrap());
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

        assert!(is_dirty(&wt.repo, &wt.path).unwrap());
    }

    #[test]
    fn is_dirty_clean_worktree_returns_false() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo).unwrap();

        let wt = create_test_wt(&repo, "sess-test", &tmp);
        // No changes
        assert!(!is_dirty(&wt.repo, &wt.path).unwrap());
    }
}
