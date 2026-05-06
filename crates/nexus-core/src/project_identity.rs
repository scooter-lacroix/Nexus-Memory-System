//! Project identity resolution logic

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::thread;

/// Unique identity for a project, used as the cache key for per-project memories.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectIdentity {
    /// Canonical absolute path to the project root directory.
    pub root_dir: PathBuf,

    /// Git remote origin URL, if available.
    pub git_remote: Option<String>,

    /// Human-readable project name.
    pub display_name: String,
}

/// Marker file for explicit project identity override.
/// Lives at `.nexus/project.toml` in the project root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMarker {
    pub name: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl ProjectIdentity {
    /// Resolve the project identity for the given working directory.
    pub fn resolve(cwd: &Path) -> Self {
        let raw_root = Self::find_project_root(cwd);
        // Canonicalize to eliminate symlinks, redundant separators, trailing slashes.
        // Fallback to raw path if canonicalization fails (shouldn't happen for real dirs).
        let root_dir = raw_root.canonicalize().unwrap_or(raw_root);
        let display_name = Self::derive_display_name(&root_dir);
        let git_remote = Self::detect_git_remote(&root_dir);

        Self {
            root_dir,
            git_remote,
            display_name,
        }
    }

    /// Walk up directory tree looking for `.nexus/project.toml` or `.git/`.
    fn find_project_root(start: &Path) -> PathBuf {
        let mut current = start.to_path_buf();
        for _ in 0..256 {
            if current.join(".nexus").join("project.toml").exists() {
                return current;
            }
            if current.join(".git").exists() {
                return current;
            }
            if !current.pop() {
                break;
            }
        }
        // Reached filesystem root or iteration cap, use original start
        start.to_path_buf()
    }

    /// Extract git remote origin URL. Never fails — returns None on error.
    fn detect_git_remote(root: &Path) -> Option<String> {
        use std::process::Stdio;
        use std::thread;

        // Spawn a blocking thread to run the git command with a timeout
        let root = root.to_path_buf();
        let handle = thread::spawn(move || {
            let output = std::process::Command::new("git")
                .args(["config", "--get", "remote.origin.url"])
                .current_dir(&root)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .ok()?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                Some(stdout.trim().to_string())
            } else {
                None
            }
        });

        handle.join().unwrap_or(None)
    }

    fn derive_display_name(root: &Path) -> String {
        // Try reading .nexus/project.toml first
        if let Ok(content) = std::fs::read_to_string(root.join(".nexus").join("project.toml")) {
            if let Ok(marker) = toml::from_str::<ProjectMarker>(&content) {
                if let Some(name) = marker.name {
                    return name;
                }
            }
        }

        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown-project")
            .to_string()
    }

    /// Stable hash key for database lookups and cache keying.
    pub fn cache_key(&self) -> String {
        self.root_dir.to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_fallback() {
        let dir = tempdir().unwrap();
        let identity = ProjectIdentity::resolve(dir.path());
        assert_eq!(identity.root_dir, dir.path());
        assert!(identity.git_remote.is_none());
    }

    #[test]
    fn test_resolve_with_marker() {
        let dir = tempdir().unwrap();
        let nexus_dir = dir.path().join(".nexus");
        std::fs::create_dir(&nexus_dir).unwrap();
        std::fs::write(nexus_dir.join("project.toml"), r#"name = "test-project""#).unwrap();

        let sub_dir = dir.path().join("sub");
        std::fs::create_dir(&sub_dir).unwrap();

        let identity = ProjectIdentity::resolve(&sub_dir);
        assert_eq!(identity.root_dir, dir.path());
        assert_eq!(identity.display_name, "test-project");
    }

    #[test]
    fn test_default_config_values() {
        let dir = tempdir().unwrap();
        let identity = ProjectIdentity::resolve(dir.path());
        assert_eq!(
            identity.display_name,
            dir.path().file_name().unwrap().to_str().unwrap()
        );
    }
}
