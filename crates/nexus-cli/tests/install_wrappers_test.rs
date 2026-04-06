use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

fn write_executable(path: &Path, content: &str) {
    // Remove existing file first to avoid "Text file busy" errors
    // when overwriting a binary that was recently executed
    if path.exists() {
        fs::remove_file(path).unwrap();
    }
    fs::write(path, content).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[test]
fn generated_wrapper_runs_session_lifecycle_and_preserves_exit_code() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let install_script = repo_root.join("scripts/install.sh");

    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let bin_dir = temp.path().join("bin");
    let tool_dir = temp.path().join("tool-bin");
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    let state_dir = temp.path().join("state");
    let logs_dir = temp.path().join("logs");

    for dir in [
        &home_dir,
        &bin_dir,
        &tool_dir,
        &config_dir,
        &data_dir,
        &state_dir,
        &logs_dir,
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    let tool_log = logs_dir.join("tool.log");
    let nexus_log = logs_dir.join("nexus.log");
    let tool_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 7\n",
        tool_log.display()
    );
    write_executable(&tool_dir.join("codex"), &tool_script);

    let install = Command::new("bash")
        .arg(install_script)
        .arg("--skip-build")
        .arg("--skip-profile")
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--state-dir")
        .arg(&state_dir)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .status()
        .unwrap();
    assert!(install.success(), "installer should succeed");

    let nexus_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
        nexus_log.display()
    );
    write_executable(&bin_dir.join("nexus"), &nexus_script);

    let wrapper = bin_dir.join("codex-nexus");
    assert!(wrapper.exists(), "codex wrapper should be generated");

    let status = Command::new(&wrapper)
        .arg("solve")
        .arg("--fast")
        .current_dir(repo_root)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .status()
        .unwrap();

    assert_eq!(
        status.code(),
        Some(7),
        "wrapper must preserve tool exit code"
    );

    let tool_invocation = fs::read_to_string(&tool_log).unwrap();
    assert!(tool_invocation.contains("solve --fast"));

    let nexus_invocation = fs::read_to_string(&nexus_log).unwrap();
    assert!(nexus_invocation.contains("session start --agent codex"));
    assert!(nexus_invocation.contains("session end --agent codex"));
    assert!(nexus_invocation.contains("--cwd"));

    let lifecycle_lines: Vec<_> = nexus_invocation.lines().collect();
    let start_index = lifecycle_lines
        .iter()
        .position(|line| line.contains("session start --agent codex"))
        .unwrap();
    let end_index = lifecycle_lines
        .iter()
        .position(|line| line.contains("session end --agent codex"))
        .unwrap();
    assert!(
        start_index < end_index,
        "session start must precede session end"
    );
}

#[test]
fn installer_accepts_explicit_binary_and_replaces_existing_nexus() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let install_script = repo_root.join("scripts/install.sh");

    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let bin_dir = temp.path().join("bin");
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    let state_dir = temp.path().join("state");
    let logs_dir = temp.path().join("logs");
    let explicit_bin = temp.path().join("nexus-release");
    let init_log = logs_dir.join("init.log");

    for dir in [
        &home_dir,
        &bin_dir,
        &config_dir,
        &data_dir,
        &state_dir,
        &logs_dir,
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    write_executable(
        &bin_dir.join("nexus"),
        "#!/usr/bin/env bash\necho old-version\n",
    );

    let explicit_binary_script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
if [[ "${{1:-}}" == "--version" ]]; then
  echo "nexus 9.9.9"
  exit 0
fi
if [[ "${{1:-}}" == "init" ]]; then
  printf 'init %s\n' "${{NEXUS_DATABASE_PATH:-}}" >> "{}"
  exit 0
fi
echo "stub nexus $*"
"#,
        init_log.display()
    );
    write_executable(&explicit_bin, &explicit_binary_script);

    let install = Command::new("bash")
        .arg(install_script)
        .arg("--skip-build")
        .arg("--skip-profile")
        .arg("--binary")
        .arg(&explicit_bin)
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--state-dir")
        .arg(&state_dir)
        .env("HOME", &home_dir)
        .status()
        .unwrap();
    assert!(install.success(), "installer should succeed");

    let installed_version = Command::new(bin_dir.join("nexus"))
        .arg("--version")
        .output()
        .unwrap();
    let installed_version = String::from_utf8(installed_version.stdout).unwrap();
    assert!(
        installed_version.contains("nexus 9.9.9"),
        "installed binary should be replaced by the explicit binary"
    );

    let init_output = fs::read_to_string(&init_log).unwrap();
    assert!(
        init_output.contains(&data_dir.join("nexus.db").display().to_string()),
        "installed binary should be used for init against the configured database"
    );
}

#[test]
fn generated_wrapper_finalizes_session_on_sigterm() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let install_script = repo_root.join("scripts/install.sh");

    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let bin_dir = temp.path().join("bin");
    let tool_dir = temp.path().join("tool-bin");
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    let state_dir = temp.path().join("state");
    let logs_dir = temp.path().join("logs");

    for dir in [
        &home_dir,
        &bin_dir,
        &tool_dir,
        &config_dir,
        &data_dir,
        &state_dir,
        &logs_dir,
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    let nexus_log = logs_dir.join("nexus-signal.log");
    let tool_script = "#!/usr/bin/env bash\nsleep 5\n";
    write_executable(&tool_dir.join("codex"), tool_script);

    let install = Command::new("bash")
        .arg(install_script)
        .arg("--skip-build")
        .arg("--skip-profile")
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--state-dir")
        .arg(&state_dir)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .status()
        .unwrap();
    assert!(install.success(), "installer should succeed");

    let nexus_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
        nexus_log.display()
    );
    write_executable(&bin_dir.join("nexus"), &nexus_script);

    let wrapper = bin_dir.join("codex-nexus");
    let mut child = Command::new(&wrapper)
        .current_dir(repo_root)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));
    let kill_status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill_status.success(), "SIGTERM should be delivered");

    let status = child.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(143),
        "wrapper should surface signal-style termination"
    );

    let nexus_invocation = fs::read_to_string(&nexus_log).unwrap();
    assert!(nexus_invocation.contains("session start --agent codex"));
    assert!(nexus_invocation.contains("session end --agent codex"));
}

#[test]
fn generated_wrapper_terminates_spawned_process_group_on_sigterm() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let install_script = repo_root.join("scripts/install.sh");

    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let bin_dir = temp.path().join("bin");
    let tool_dir = temp.path().join("tool-bin");
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    let state_dir = temp.path().join("state");
    let logs_dir = temp.path().join("logs");

    for dir in [
        &home_dir,
        &bin_dir,
        &tool_dir,
        &config_dir,
        &data_dir,
        &state_dir,
        &logs_dir,
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    let nexus_log = logs_dir.join("nexus-group.log");
    let grandchild_log = logs_dir.join("grandchild.log");
    let tool_script = format!(
        "#!/usr/bin/env bash\nchild_log=\"{}\"\n(\n  trap 'printf \"terminated\\n\" >> \"$child_log\"; exit 0' TERM INT\n  while true; do sleep 1; done\n) &\nworker=$!\nwait \"$worker\"\n",
        grandchild_log.display()
    );
    write_executable(&tool_dir.join("codex"), &tool_script);

    let install = Command::new("bash")
        .arg(install_script)
        .arg("--skip-build")
        .arg("--skip-profile")
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--state-dir")
        .arg(&state_dir)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .status()
        .unwrap();
    assert!(install.success(), "installer should succeed");

    let nexus_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
        nexus_log.display()
    );
    write_executable(&bin_dir.join("nexus"), &nexus_script);

    let wrapper = bin_dir.join("codex-nexus");
    let mut child = Command::new(&wrapper)
        .current_dir(repo_root)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(200));
    let kill_status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill_status.success(), "SIGTERM should be delivered");

    let status = child.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(143),
        "wrapper should surface signal-style termination"
    );

    thread::sleep(Duration::from_millis(200));
    let grandchild_state = fs::read_to_string(&grandchild_log).unwrap();
    assert!(
        grandchild_state.contains("terminated"),
        "spawned descendants should receive the termination signal"
    );

    let nexus_invocation = fs::read_to_string(&nexus_log).unwrap();
    assert!(nexus_invocation.contains("session end --agent codex"));
}

#[test]
fn generated_wrapper_supports_hermes_generic_cli_lifecycle() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let install_script = repo_root.join("scripts/install.sh");

    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let bin_dir = temp.path().join("bin");
    let tool_dir = temp.path().join("tool-bin");
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    let state_dir = temp.path().join("state");
    let logs_dir = temp.path().join("logs");

    for dir in [
        &home_dir,
        &bin_dir,
        &tool_dir,
        &config_dir,
        &data_dir,
        &state_dir,
        &logs_dir,
    ] {
        fs::create_dir_all(dir).unwrap();
    }

    let tool_log = logs_dir.join("hermes-tool.log");
    let nexus_log = logs_dir.join("hermes-nexus.log");
    let tool_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
        tool_log.display()
    );
    write_executable(&tool_dir.join("hermes"), &tool_script);

    let install = Command::new("bash")
        .arg(install_script)
        .arg("--skip-build")
        .arg("--skip-profile")
        .arg("--bin-dir")
        .arg(&bin_dir)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--state-dir")
        .arg(&state_dir)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .status()
        .unwrap();
    assert!(install.success(), "installer should succeed");

    let nexus_script = format!(
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
        nexus_log.display()
    );
    write_executable(&bin_dir.join("nexus"), &nexus_script);

    let wrapper = bin_dir.join("hermes-nexus");
    assert!(wrapper.exists(), "hermes wrapper should be generated");

    let status = Command::new(&wrapper)
        .arg("prompt")
        .arg("memory-check")
        .current_dir(repo_root)
        .env("HOME", &home_dir)
        .env(
            "PATH",
            format!("{}:{}", tool_dir.display(), std::env::var("PATH").unwrap()),
        )
        .status()
        .unwrap();
    assert!(status.success(), "hermes wrapper should return tool status");

    let tool_invocation = fs::read_to_string(&tool_log).unwrap();
    assert!(tool_invocation.contains("prompt memory-check"));

    let nexus_invocation = fs::read_to_string(&nexus_log).unwrap();
    assert!(nexus_invocation.contains("session start --agent hermes"));
    assert!(nexus_invocation.contains("session end --agent hermes"));
}
