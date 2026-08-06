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

fn git_out(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").args(args).output().map_err(|e| e.to_string())?;
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

/// Toplevel of the git repo containing `dir`, if any.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let d = dir.to_string_lossy().to_string();
    git_out(&["-C", &d, "rev-parse", "--show-toplevel"])
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Where worktrees are kept — outside any repo.
fn base_dir() -> PathBuf {
    let root = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string());
    PathBuf::from(root).join("mosaic").join("worktrees")
}

/// Create an isolated worktree + branch for a session. The branch carries a short
/// unique suffix so re-used session ids across app runs never collide.
pub fn create(repo: &Path, session_id: &str) -> Result<Worktree, String> {
    let uid = uuid::Uuid::new_v4().simple().to_string();
    let name = format!("{session_id}-{}", &uid[..6]);
    let branch = format!("mosaic/{name}");
    let path = base_dir().join(&name);
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

/// Remove the worktree. The branch is deleted ONLY if it has no commits of its
/// own — we never silently discard an agent's work.
pub fn remove(wt: &Worktree) {
    let repo = wt.repo.to_string_lossy().to_string();
    let path = wt.path.to_string_lossy().to_string();
    let _ = git_ok(&["-C", &repo, "worktree", "remove", "--force", &path]);

    let range = format!("{}..{}", wt.base, wt.branch);
    let unique = git_out(&["-C", &repo, "rev-list", "--count", &range]).unwrap_or_else(|_| "1".into());
    if unique.trim() == "0" {
        let _ = git_ok(&["-C", &repo, "branch", "-D", &wt.branch]);
    }
    let _ = git_ok(&["-C", &repo, "worktree", "prune"]);
}
