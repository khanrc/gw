use crate::GwError;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone)]
pub struct Git;

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

impl Git {
    pub fn new() -> Self {
        Self
    }

    pub fn repo_root(&self) -> Result<PathBuf, String> {
        let toplevel = self.run(&["rev-parse", "--show-toplevel"])?;
        let toplevel_path = PathBuf::from(toplevel.trim());
        let common = self.run(&["rev-parse", "--git-common-dir"])?;
        let mut common_path = PathBuf::from(common.trim());
        if common_path.is_relative() {
            // --git-common-dir returns a path relative to CWD, not toplevel
            let cwd = std::env::current_dir()
                .map_err(|e| format!("failed to get current directory: {}", e))?;
            common_path = cwd.join(common_path);
        }
        common_path = common_path.canonicalize().unwrap_or(common_path);
        let root = root_from_common_dir(&common_path).unwrap_or(toplevel_path);
        Ok(root)
    }

    pub fn current_toplevel(&self) -> Result<PathBuf, String> {
        let out = self.run(&["rev-parse", "--show-toplevel"])?;
        Ok(PathBuf::from(out.trim()))
    }

    pub fn run(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .args(args)
            .output()
            .map_err(|e| format!("git execution failed: {}", e))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .map_err(|e| format!("git execution failed: {}", e))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn worktrees(&self) -> Result<Vec<Worktree>, String> {
        let out = self.run(&["worktree", "list", "--porcelain"])?;
        let mut result = Vec::new();
        let mut current: Option<Worktree> = None;
        for line in out.lines() {
            if line.starts_with("worktree ") {
                if let Some(wt) = current.take() {
                    result.push(wt);
                }
                let path = line.trim_start_matches("worktree ");
                current = Some(Worktree {
                    path: PathBuf::from(path),
                    branch: None,
                    head: None,
                });
            } else if line.starts_with("branch ") {
                if let Some(ref mut wt) = current {
                    let branch = line.trim_start_matches("branch ");
                    wt.branch = Some(branch.trim().to_string());
                }
            } else if line.starts_with("HEAD ") {
                if let Some(ref mut wt) = current {
                    wt.head = Some(line.trim_start_matches("HEAD ").trim().to_string());
                }
            }
        }
        if let Some(wt) = current {
            result.push(wt);
        }
        Ok(result)
    }

    pub fn branch_exists(&self, branch: &str) -> bool {
        self.run(&["show-ref", "--verify", &format!("refs/heads/{}", branch)])
            .is_ok()
    }

    pub fn current_branch(&self, repo_root: &Path) -> Result<String, String> {
        let out = self.run_in(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        Ok(out.trim().to_string())
    }

    pub fn resolve_base(&self, repo_root: &Path, default_base: Option<String>) -> Result<String, String> {
        if let Some(base) = default_base {
            return Ok(base);
        }
        if let Ok(out) = self.run(&["symbolic-ref", "refs/remotes/origin/HEAD"]) {
            let branch = out.trim().trim_start_matches("refs/remotes/origin/");
            if !branch.is_empty() {
                return Ok(branch.to_string());
            }
        }
        if self
            .run(&["show-ref", "--verify", "refs/heads/main"])
            .is_ok()
            || self
                .run(&["show-ref", "--verify", "refs/remotes/origin/main"])
                .is_ok()
        {
            return Ok("main".to_string());
        }
        if self
            .run(&["show-ref", "--verify", "refs/heads/master"])
            .is_ok()
            || self
                .run(&["show-ref", "--verify", "refs/remotes/origin/master"])
                .is_ok()
        {
            return Ok("master".to_string());
        }
        self.current_branch(repo_root)
    }
}

pub fn git_error(msg: impl Into<String>) -> GwError {
    GwError::new(2, msg)
}

fn root_from_common_dir(common: &Path) -> Option<PathBuf> {
    for ancestor in common.ancestors() {
        if ancestor.file_name().map(|n| n == ".git").unwrap_or(false) {
            return ancestor.parent().map(|p| p.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_common_dir_from_subdirectory() {
        // Simulates the fix: --git-common-dir relative path must be joined
        // to CWD (not toplevel) before canonicalize.
        let tmp = std::env::temp_dir().join("gw_test_subdir_root");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let git = Git::new();
        git.run_in(&tmp, &["init"]).unwrap();
        git.run_in(&tmp, &["commit", "--allow-empty", "-m", "init"]).unwrap();

        let subdir = tmp.join("services").join("app");
        fs::create_dir_all(&subdir).unwrap();

        // Get --git-common-dir from the subdirectory (relative to CWD=subdir)
        let output = Command::new("git")
            .current_dir(&subdir)
            .args(["rev-parse", "--git-common-dir"])
            .output()
            .unwrap();
        let common_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let common_path = PathBuf::from(&common_str);

        // BUG: joining relative common_path with toplevel gives wrong result
        let toplevel = tmp.canonicalize().unwrap();
        let wrong = toplevel.join(&common_path);
        let wrong_resolved = root_from_common_dir(&wrong);
        // The wrong path has unresolved '..' so it won't match the repo root
        assert_ne!(wrong_resolved, Some(toplevel.clone()),
            "joining to toplevel should NOT produce the correct root (this is the bug)");

        // FIX: joining relative common_path with CWD (subdir) gives correct result
        let correct = subdir.join(&common_path);
        let correct = correct.canonicalize().unwrap();
        let correct_root = root_from_common_dir(&correct).unwrap();
        assert_eq!(correct_root, toplevel,
            "joining to CWD + canonicalize should produce the correct repo root");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn root_from_common_dir_canonical_path() {
        // .git path without any '..' should work
        let path = PathBuf::from("/home/user/project/.git");
        assert_eq!(root_from_common_dir(&path), Some(PathBuf::from("/home/user/project")));
    }

    #[test]
    fn root_from_common_dir_no_git() {
        let path = PathBuf::from("/home/user/project");
        assert_eq!(root_from_common_dir(&path), None);
    }
}
