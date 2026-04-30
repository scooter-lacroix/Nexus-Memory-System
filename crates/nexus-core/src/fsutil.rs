//! Filesystem utilities shared across crates.

use std::io::{ErrorKind, Write};
use std::path::Path;

/// Write a file atomically: write to a temp file, sync, then rename.
/// Prevents partial writes on crash. Uses PID-scoped tmp to avoid
/// collision when concurrent processes write the same target.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp_path = path.with_extension(format!(
        "tmp.{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }

    let result = match std::fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            // Some mounted filesystems do not replace an existing destination
            // during rename. Fall back to a remove-and-replace flow so writes
            // remain idempotent on overwrite-hostile mounts.
            match std::fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_dir() => Err(err),
                Ok(_) => {
                    let _ = std::fs::remove_file(path);
                    std::fs::rename(&tmp_path, path)
                }
                Err(_) => std::fs::rename(&tmp_path, path),
            }
        }
        Err(err) => Err(err),
    };

    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("content.md");

        std::fs::write(&path, "old").unwrap();
        atomic_write(&path, "new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }
}
