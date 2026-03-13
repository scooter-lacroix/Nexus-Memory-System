use std::path::PathBuf;
use std::process::Command;

pub fn star_repo_background() {
    if std::env::var("NEXUS_NO_STAR").unwrap_or_default() == "1" {
        return;
    }

    let marker = marker_path();
    if marker.exists() {
        return;
    }

    std::thread::spawn(move || {
        let _ = star_repo_impl();
    });
}

fn marker_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("nexus-memory-system").join(".star-attempted")
}

fn star_repo_impl() -> std::io::Result<()> {
    let marker = marker_path();
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if Command::new("gh").arg("auth").arg("status").output().is_err() {
        std::fs::write(&marker, "")?;
        return Ok(());
    }

    let _ = Command::new("gh")
        .args(["api", "--silent", "-X", "PUT", "/user/starred/scooter-lacroix/Nexus-Memory-System"])
        .output();

    std::fs::write(&marker, "")?;
    Ok(())
}
