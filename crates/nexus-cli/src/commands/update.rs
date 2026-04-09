//! `nexus update` command — self-update via the detected install method.

use std::path::PathBuf;
use std::process::Command;

/// Metadata file written by the install script to record how nexus was installed.
const INSTALL_METHOD_FILE: &str = "install-method";

/// Possible installation methods.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallMethod {
    /// `cargo install nexus-memory`
    Cargo,
    /// `scripts/install.sh` (or curl | bash)
    InstallScript,
    /// Downloaded binary from GitHub releases
    GitRelease,
    /// Could not determine
    Unknown,
}

impl std::fmt::Display for InstallMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallMethod::Cargo => write!(f, "cargo install"),
            InstallMethod::InstallScript => write!(f, "install script"),
            InstallMethod::GitRelease => write!(f, "GitHub release"),
            InstallMethod::Unknown => write!(f, "unknown"),
        }
    }
}

/// Default data directory (matches install.sh default).
fn default_data_dir() -> Option<PathBuf> {
    if let Ok(data_dir) = std::env::var("NEXUS_INSTALL_DATA_DIR") {
        return Some(PathBuf::from(data_dir));
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return Some(PathBuf::from(xdg).join("nexus-memory-system"));
    }
    dirs::home_dir().map(|h| h.join(".local").join("share").join("nexus-memory-system"))
}

/// Detect how nexus was installed.
fn detect_install_method() -> InstallMethod {
    // 1. Check for install-method metadata file written by install.sh
    if let Some(data_dir) = default_data_dir() {
        let method_file = data_dir.join(INSTALL_METHOD_FILE);
        if let Ok(content) = std::fs::read_to_string(&method_file) {
            let method = content.trim().to_lowercase();
            match method.as_str() {
                "cargo" => return InstallMethod::Cargo,
                "install-script" | "install_script" | "script" => {
                    return InstallMethod::InstallScript
                }
                "git-release" | "git_release" | "github" => return InstallMethod::GitRelease,
                _ => {}
            }
        }
    }

    // 2. Check if binary is in CARGO_HOME/bin (cargo install)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(cargo_home) = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".cargo")))
        {
            let cargo_bin = cargo_home.join("bin");
            if exe.starts_with(&cargo_bin) {
                // Check if there's a .cargo-install-marker or if `cargo install` metadata exists
                // The simplest heuristic: if the binary is in .cargo/bin, assume cargo install
                if is_cargo_install(&exe) {
                    return InstallMethod::Cargo;
                }
            }
        }

        // 3. Check for install script markers (binary in ~/.local/bin or data dir exists)
        if let Some(data_dir) = default_data_dir() {
            if data_dir.exists() {
                // If data dir exists but no install-method file, check binary location
                if let Some(local_bin) = dirs::home_dir().map(|h| h.join(".local").join("bin")) {
                    if exe.starts_with(&local_bin) {
                        return InstallMethod::InstallScript;
                    }
                }
            }
        }
    }

    InstallMethod::Unknown
}

/// Check if the binary at the given path was installed via `cargo install`.
fn is_cargo_install(_exe: &PathBuf) -> bool {
    // cargo install creates a `.d` directory with metadata next to the binary
    // on some platforms. A simpler check: try `cargo install --list` and see if
    // nexus-memory appears.
    let output = Command::new("cargo").args(["install", "--list"]).output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("nexus-memory")
        }
        _ => false,
    }
}

/// Get the current version of the nexus binary.
fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Get the latest version from crates.io.
fn latest_crates_io_version() -> Result<String, String> {
    let output = Command::new("cargo")
        .args(["search", "nexus-memory", "--limit", "1"])
        .output()
        .map_err(|e| format!("Failed to run cargo search: {}", e))?;

    if !output.status.success() {
        return Err("cargo search failed".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "nexus-memory = \"1.2.3\" ..."
    for line in stdout.lines() {
        if line.starts_with("nexus-memory =") {
            if let Some(version) = line.split('"').nth(1) {
                return Ok(version.to_string());
            }
        }
    }

    Err("Could not parse version from cargo search output".to_string())
}

/// Get the latest version from GitHub releases.
fn latest_github_version() -> Result<String, String> {
    let output = Command::new("gh")
        .args([
            "release",
            "list",
            "--repo",
            "scooter-lacroix/Nexus-Memory-System",
            "--limit",
            "1",
        ])
        .output()
        .map_err(|e| format!("Failed to run gh release list: {}. Is gh CLI installed?", e))?;

    if !output.status.success() {
        return Err("gh release list failed".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "v1.2.3  ..."
    if let Some(first_line) = stdout.lines().next() {
        let tag = first_line.split_whitespace().next().unwrap_or("");
        if let Some(version) = tag.strip_prefix('v') {
            return Ok(version.to_string());
        }
    }

    Err("Could not parse version from gh release list".to_string())
}

/// Run the update via cargo install.
fn update_via_cargo() -> Result<(), String> {
    println!("Updating nexus-memory via cargo install...");

    let status = Command::new("cargo")
        .args(["install", "nexus-memory", "--force"])
        .status()
        .map_err(|e| format!("Failed to run cargo install: {}", e))?;

    if status.success() {
        println!("Updated successfully via cargo install.");
        Ok(())
    } else {
        Err("cargo install failed".to_string())
    }
}

/// Run the update via the install script.
fn update_via_install_script() -> Result<(), String> {
    println!("Updating nexus via install script...");

    // Try local repo first, then fall back to curl
    let script_path = find_repo_root()
        .map(|root| root.join("scripts/install.sh"))
        .filter(|p| p.exists());

    if let Some(path) = script_path {
        let status = Command::new("bash")
            .arg(&path)
            .status()
            .map_err(|e| format!("Failed to run install script: {}", e))?;

        if status.success() {
            println!("Updated successfully via install script.");
            return Ok(());
        } else {
            return Err("Install script failed".to_string());
        }
    }

    // Fall back to downloading from GitHub
    let tmp = std::env::temp_dir().join("nexus-install-update.sh");
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "https://raw.githubusercontent.com/scooter-lacroix/Nexus-Memory-System/master/scripts/install.sh",
            "-o",
        ])
        .arg(&tmp)
        .status()
        .map_err(|e| format!("curl failed: {}", e))?;

    if !status.success() {
        return Err("Failed to download install script".to_string());
    }

    let status = Command::new("bash")
        .arg(&tmp)
        .status()
        .map_err(|e| format!("Failed to run install script: {}", e))?;

    if status.success() {
        println!("Updated successfully via install script.");
        Ok(())
    } else {
        Err("Install script failed".to_string())
    }
}

/// Run the update via GitHub release download.
fn update_via_git_release() -> Result<(), String> {
    println!("Updating nexus via GitHub release...");

    // Find the latest release tag
    let output = Command::new("gh")
        .args([
            "release",
            "list",
            "--repo",
            "scooter-lacroix/Nexus-Memory-System",
            "--limit",
            "1",
        ])
        .output()
        .map_err(|e| format!("Failed to run gh: {}. Is gh CLI installed?", e))?;

    if !output.status.success() {
        return Err("gh release list failed".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let tag = stdout
        .split_whitespace()
        .next()
        .ok_or("No releases found")?;

    // Determine the binary name for the current platform
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    };
    let asset_name = format!("nexus-{}-{}.tar.gz", os, arch);

    // Download the release
    let tmp_dir = std::env::temp_dir().join("nexus-update");
    std::fs::create_dir_all(&tmp_dir).ok();

    let status = Command::new("gh")
        .args([
            "release",
            "download",
            tag,
            "--repo",
            "scooter-lacroix/Nexus-Memory-System",
            "--pattern",
            &asset_name,
            "--dir",
        ])
        .arg(&tmp_dir)
        .status()
        .map_err(|e| format!("Failed to download release: {}", e))?;

    if !status.success() {
        return Err(format!(
            "Failed to download asset '{}'. It may not exist for your platform.",
            asset_name
        ));
    }

    // Extract and install
    let archive = tmp_dir.join(&asset_name);
    let install_dir = dirs::home_dir()
        .map(|h| h.join(".local").join("bin"))
        .ok_or("Cannot determine install directory")?;

    std::fs::create_dir_all(&install_dir).ok();

    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(&archive)
        .args(["-C"])
        .arg(&install_dir)
        .status()
        .map_err(|e| format!("Failed to extract: {}", e))?;

    if status.success() {
        println!("Updated to {} via GitHub release.", tag);
        Ok(())
    } else {
        Err("Failed to extract release archive".to_string())
    }
}

/// Try to find the repo root by looking for Cargo.toml in parent dirs of the binary.
fn find_repo_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?.parent()?;
    let cargo_toml = dir.join("Cargo.toml");
    if cargo_toml.exists() {
        Some(dir.to_path_buf())
    } else {
        None
    }
}

/// Execute the update command.
pub async fn execute(check: bool) -> anyhow::Result<()> {
    let current = current_version();
    println!("Current version: {}", current);

    let method = detect_install_method();
    println!("Install method: {}", method);

    if check {
        // Only check for updates, don't install
        let latest = match method {
            InstallMethod::Cargo => latest_crates_io_version(),
            _ => latest_github_version(),
        };

        match latest {
            Ok(latest) => {
                println!("Latest version: {}", latest);
                if latest == current {
                    println!("Already up to date.");
                } else {
                    println!("Update available: {} -> {}", current, latest);
                }
            }
            Err(e) => {
                println!("Could not check for updates: {}", e);
            }
        }
        return Ok(());
    }

    // Perform the update
    let result = match method {
        InstallMethod::Cargo => update_via_cargo(),
        InstallMethod::InstallScript => update_via_install_script(),
        InstallMethod::GitRelease => update_via_git_release(),
        InstallMethod::Unknown => {
            // Try cargo install as the default
            println!("Could not determine install method. Trying cargo install...");
            update_via_cargo()
        }
    };

    match result {
        Ok(()) => {
            // Verify the new version
            let output = Command::new("nexus").arg("--version").output();
            match output {
                Ok(o) if o.status.success() => {
                    let new_version = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    println!("New version: {}", new_version);
                }
                _ => {
                    println!("Update completed but could not verify new version.");
                }
            }
        }
        Err(e) => {
            eprintln!("Update failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
