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
            // during rename. Move the old file aside first so we can restore it
            // if the replacement rename fails. This is not fully atomic, but it
            // preserves the previous contents across crashes and errors.
            match std::fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_dir() => Err(err),
                Ok(_) => {
                    let backup_path = path.with_extension(format!(
                        "bak.{}-{}",
                        std::process::id(),
                        uuid::Uuid::new_v4()
                    ));
                    match std::fs::rename(path, &backup_path) {
                        Ok(()) => match std::fs::rename(&tmp_path, path) {
                            Ok(()) => {
                                let _ = std::fs::remove_file(&backup_path);
                                Ok(())
                            }
                            Err(rename_err) => {
                                let _ = std::fs::rename(&backup_path, path);
                                Err(rename_err)
                            }
                        },
                        Err(backup_err) => Err(backup_err),
                    }
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
