//! Install-origin aware `nexus update` command.

use anyhow::{anyhow, Context, Result};
use semver::Version;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

const GITHUB_REPO: &str = "scooter-lacroix/Nexus-Memory-System";
const INSTALL_METHOD_FILE_DEFAULT: &str = "install-method";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMethod {
    CargoInstall,
    InstallScript,
    GitHubRelease,
}

impl InstallMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::CargoInstall => "cargo-install",
            Self::InstallScript => "install-script",
            Self::GitHubRelease => "github-release",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "cargo-install" => Some(Self::CargoInstall),
            "install-script" => Some(Self::InstallScript),
            "github-release" => Some(Self::GitHubRelease),
            _ => None,
        }
    }
}

pub async fn execute(check_only: bool) -> Result<()> {
    ensure_gh_available()?;

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("Failed to parse current binary version as semver")?;
    let latest = latest_github_version()?;

    println!("Current version: {}", current);
    println!("Latest version:  {}", latest);

    if latest <= current {
        println!("Already up to date.");
        return Ok(());
    }

    if check_only {
        println!("Update available.");
        return Ok(());
    }

    let current_exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let method = detect_install_method(&current_exe);
    println!("Detected install method: {}", method.as_str());

    match method {
        InstallMethod::CargoInstall => update_via_cargo_install(),
        InstallMethod::InstallScript | InstallMethod::GitHubRelease => {
            update_via_verified_release_download(&latest, &current_exe, method)
        }
    }
}

fn latest_github_version() -> Result<Version> {
    let output = Command::new("gh")
        .args([
            "release",
            "view",
            "--repo",
            GITHUB_REPO,
            "--json",
            "tagName",
            "--jq",
            ".tagName",
        ])
        .output()
        .context(
            "Failed to execute `gh release view` to get latest release. \
Install and authenticate GitHub CLI first: `gh auth login`",
        )?;

    if !output.status.success() {
        return Err(anyhow!(
            "`gh release view` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let tag = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_version_tag(&tag)
}

fn parse_version_tag(tag: &str) -> Result<Version> {
    let normalized = tag.trim().trim_start_matches('v');
    Version::parse(normalized).map_err(|e| anyhow!("Invalid release tag '{}': {}", tag, e))
}

fn ensure_gh_available() -> Result<()> {
    let which = Command::new("gh").arg("--version").output();
    match which {
        Ok(output) if output.status.success() => {}
        Ok(_) | Err(_) => {
            return Err(anyhow!(
                "GitHub CLI (`gh`) is required for `nexus update`. Install it and run `gh auth login`."
            ));
        }
    }

    let auth = Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map_err(|_| {
            anyhow!("Failed to run `gh auth status`. Authenticate first with `gh auth login`.")
        })?;
    if !auth.status.success() {
        return Err(anyhow!(
            "`gh` is not authenticated. Run `gh auth login` and retry."
        ));
    }

    Ok(())
}

fn install_method_filename() -> String {
    std::env::var("INSTALL_METHOD_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| INSTALL_METHOD_FILE_DEFAULT.to_string())
}

fn detect_install_method(exe: &Path) -> InstallMethod {
    if let Some(parent) = exe.parent() {
        let marker = parent.join(install_method_filename());
        if let Ok(content) = fs::read_to_string(marker) {
            if let Some(method) = InstallMethod::parse(&content) {
                return method;
            }
        }
    }

    if is_cargo_install(exe) {
        InstallMethod::CargoInstall
    } else {
        InstallMethod::GitHubRelease
    }
}

fn is_cargo_install(exe: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    exe.starts_with(home.join(".cargo").join("bin"))
}

fn update_via_cargo_install() -> Result<()> {
    println!("Updating with cargo install...");
    let status = Command::new("cargo")
        .args(["install", "nexus-memory", "--locked", "--force"])
        .status()
        .context("Failed to run cargo install")?;

    if !status.success() {
        return Err(anyhow!("cargo install failed with status {}", status));
    }

    println!("Update completed via cargo install.");
    Ok(())
}

fn update_via_verified_release_download(
    latest: &Version,
    current_exe: &Path,
    detected_method: InstallMethod,
) -> Result<()> {
    let tag = format!("v{}", latest);
    let assets = release_assets(&tag)?;
    let asset_name = select_release_asset_for_current_platform(&assets, &tag)?;
    let checksums_asset = select_release_checksums_asset(&assets, &asset_name, &tag)?;
    let temp = tempdir().context("Failed to create temporary directory")?;
    let temp_path = temp.path();

    download_release_asset(&tag, &asset_name, temp_path)?;
    download_release_asset(&tag, &checksums_asset, temp_path)?;

    let archive_path = temp_path.join(&asset_name);
    let checksums_path = temp_path.join(&checksums_asset);
    verify_asset_checksum(&archive_path, &checksums_path)?;

    let extract_dir = temp_path.join("extract");
    fs::create_dir_all(&extract_dir).context("Failed to create extraction directory")?;
    extract_tarball(&archive_path, &extract_dir)?;

    let installed_bin = find_extracted_nexus_binary(&extract_dir)?;
    install_binary_and_method_file(&installed_bin, current_exe, detected_method)?;

    println!("Update completed from GitHub release {}.", tag);
    Ok(())
}

fn select_release_asset_for_current_platform(
    assets: &[serde_json::Value],
    tag: &str,
) -> Result<String> {
    let platform_tokens = os_tokens(std::env::consts::OS);
    let arch_tokens = arch_tokens(std::env::consts::ARCH);

    let mut candidates = assets
        .iter()
        .filter_map(|value| value.get("name").and_then(|name| name.as_str()))
        .filter(|name| name.ends_with(".tar.gz"))
        .filter(|name| platform_tokens.iter().any(|token| name.contains(token)))
        .filter(|name| arch_tokens.iter().any(|token| name.contains(token)))
        .map(|name| name.to_string())
        .collect::<Vec<_>>();

    candidates.sort();
    candidates.into_iter().next().ok_or_else(|| {
        anyhow!(
            "No release asset found for {}-{} at tag {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            tag
        )
    })
}

fn select_release_checksums_asset(
    assets: &[serde_json::Value],
    selected_archive: &str,
    tag: &str,
) -> Result<String> {
    let stem = selected_archive.trim_end_matches(".tar.gz");
    if let Some(exact) = assets
        .iter()
        .filter_map(|value| value.get("name").and_then(|name| name.as_str()))
        .find(|name| {
            (name.contains("checksum") || name.contains("sha256"))
                && (name.contains(stem) || name.contains(selected_archive))
        })
    {
        return Ok(exact.to_string());
    }

    assets
        .iter()
        .filter_map(|value| value.get("name").and_then(|name| name.as_str()))
        .find(|name| name.contains("checksum") || name.contains("sha256"))
        .map(|name| name.to_string())
        .ok_or_else(|| anyhow!("No checksum asset found at tag {}", tag))
}

fn release_assets(tag: &str) -> Result<Vec<serde_json::Value>> {
    let output = Command::new("gh")
        .args([
            "release",
            "view",
            tag,
            "--repo",
            GITHUB_REPO,
            "--json",
            "assets",
            "--jq",
            ".assets",
        ])
        .output()
        .context(
            "Failed to fetch release assets via `gh`. \
Install and authenticate GitHub CLI first: `gh auth login`",
        )?;

    if !output.status.success() {
        return Err(anyhow!(
            "`gh release view` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout)
        .context("Failed to parse release asset list JSON")
}

fn download_release_asset(tag: &str, asset_name: &str, output_dir: &Path) -> Result<()> {
    let status = Command::new("gh")
        .args([
            "release",
            "download",
            tag,
            "--repo",
            GITHUB_REPO,
            "--pattern",
            asset_name,
            "--dir",
        ])
        .arg(output_dir)
        .status()
        .with_context(|| format!("Failed to download release asset {}", asset_name))?;

    if !status.success() {
        return Err(anyhow!(
            "Downloading release asset {} failed with status {}",
            asset_name,
            status
        ));
    }

    Ok(())
}

fn verify_asset_checksum(archive_path: &Path, checksums_path: &Path) -> Result<()> {
    let checksums = fs::read_to_string(checksums_path)
        .with_context(|| format!("Failed to read checksums file {}", checksums_path.display()))?;
    let expected = checksums
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            let basename = Path::new(name).file_name()?.to_str()?;
            if basename == archive_path.file_name()?.to_str()? {
                Some(hash.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            anyhow!(
                "Checksum for {} not found in {}",
                archive_path.display(),
                checksums_path.display()
            )
        })?;

    let mut file = fs::File::open(archive_path)
        .with_context(|| format!("Failed to open {}", archive_path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());

    if actual != expected {
        return Err(anyhow!(
            "Checksum mismatch for {} (expected {}, got {})",
            archive_path.display(),
            expected,
            actual
        ));
    }

    Ok(())
}

fn extract_tarball(archive_path: &Path, destination: &Path) -> Result<()> {
    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(archive_path)
        .args(["-C"])
        .arg(destination)
        .status()
        .with_context(|| format!("Failed to extract archive {}", archive_path.display()))?;

    if !status.success() {
        return Err(anyhow!("Tar extraction failed with status {}", status));
    }
    Ok(())
}

fn find_extracted_nexus_binary(extract_dir: &Path) -> Result<PathBuf> {
    let mut stack = vec![extract_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .with_context(|| format!("Failed to read extracted dir {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
                if file_name == "nexus" || file_name == "nexus-bin" {
                    return Ok(path);
                }
            }
        }
    }

    Err(anyhow!(
        "No nexus binary found in extracted release archive"
    ))
}

fn install_binary_and_method_file(
    new_binary: &Path,
    current_exe: &Path,
    method: InstallMethod,
) -> Result<()> {
    let target_bin = if let Some(parent) = current_exe.parent() {
        parent.join("nexus")
    } else {
        current_exe.to_path_buf()
    };

    let unique = format!(
        ".nexus-update-{}.tmp",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let temp_target = target_bin.with_file_name(format!(
        "{}{}",
        target_bin
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("nexus"),
        unique
    ));

    fs::copy(new_binary, &temp_target).with_context(|| {
        format!(
            "Failed to install updated binary from {} to {}",
            new_binary.display(),
            temp_target.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_target)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_target, perms)?;
    }

    if let Err(err) = fs::rename(&temp_target, &target_bin) {
        let _ = fs::remove_file(&temp_target);
        return Err(err).with_context(|| {
            format!(
                "Failed to replace binary at {} with {}",
                target_bin.display(),
                temp_target.display()
            )
        });
    }

    if let Some(parent) = target_bin.parent() {
        let marker_path = parent.join(install_method_filename());
        fs::write(&marker_path, format!("{}\n", method.as_str())).with_context(|| {
            format!(
                "Failed to write install method marker {}",
                marker_path.display()
            )
        })?;
    }

    Ok(())
}

fn os_tokens(os: &str) -> Vec<String> {
    match os {
        "macos" => vec![
            "darwin".to_string(),
            "apple-darwin".to_string(),
            "macos".to_string(),
        ],
        "linux" => vec!["linux".to_string(), "unknown-linux".to_string()],
        "windows" => vec!["windows".to_string(), "pc-windows".to_string()],
        other => vec![other.to_string()],
    }
}

fn arch_tokens(arch: &str) -> Vec<String> {
    match arch {
        "aarch64" => vec!["aarch64".to_string(), "arm64".to_string()],
        "x86_64" => vec!["x86_64".to_string(), "amd64".to_string()],
        "x86" => vec!["x86".to_string(), "i686".to_string()],
        other => vec![other.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_tag_handles_v_prefix() {
        let parsed = parse_version_tag("v1.2.3").unwrap();
        assert_eq!(parsed, Version::parse("1.2.3").unwrap());
    }

    #[test]
    fn detect_install_method_uses_marker_file() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("nexus");
        fs::write(&exe, b"").unwrap();
        fs::write(
            dir.path().join(INSTALL_METHOD_FILE_DEFAULT),
            b"github-release\n",
        )
        .unwrap();

        assert_eq!(detect_install_method(&exe), InstallMethod::GitHubRelease);
    }

    #[test]
    fn install_method_parse_rejects_unknown() {
        assert_eq!(InstallMethod::parse("nope"), None);
        assert_eq!(
            InstallMethod::parse("cargo-install"),
            Some(InstallMethod::CargoInstall)
        );
    }
}
