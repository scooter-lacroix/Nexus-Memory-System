//! Filesystem utilities shared across crates.

use std::io::Write;
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
    let result = std::fs::rename(&tmp_path, path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}
