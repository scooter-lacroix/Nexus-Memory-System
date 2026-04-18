//! Filesystem utilities shared across crates.

use std::io::Write;
use std::path::Path;

/// Write a file atomically: write to a temp file, sync, then rename.
/// Prevents partial writes on crash.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)
}
