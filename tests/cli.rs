use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BIN: &str = env!("CARGO_BIN_EXE_protonhax");

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("protonhax-cli-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn prints_version_with_short_and_long_flags() {
    let expected = format!("protonhax {}\n", env!("CARGO_PKG_VERSION"));
    for flag in ["-v", "--version"] {
        let output = successful(Command::new(BIN).arg(flag));
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[test]
fn prints_greeting_with_version_without_command() {
    let output = successful(&mut Command::new(BIN));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with(&format!("protonhax {}\n", env!("CARGO_PKG_VERSION"))));
    assert!(stdout.contains("Commands:"));
    assert!(stdout.contains("  run          Runs <cmd> as a Windows application"));
}

#[test]
fn complete_cli_workflow() {
    let directory = TestDirectory::new().unwrap();
    let runtime = directory.0.join("run");
    let config = directory.0.join("config");
    let home = directory.0.join("home");
    let compat = directory.0.join("compat");
    fs::create_dir_all(compat.join("pfx")).unwrap();
    fs::create_dir_all(home.join(".var/app/com.valvesoftware.Steam")).unwrap();
    fs::create_dir(&runtime).unwrap();
    fs::create_dir(&config).unwrap();

    let proton = directory.0.join("proton");
    write_executable(
        &proton,
        "#!/bin/sh\nif [ \"${1-}\" = run ]; then shift; fi\nexec \"$@\"\n",
    );
    let helper = directory.0.join("helper.sh");
    write_executable(
        &helper,
        "#!/bin/sh\nprintf '%s\\n%s\\n%s\\n' \"$RUN_MARKER\" \"$(pwd)\" \"${REMOVED-unset}\" > \"$RUN_OUTPUT\"\n",
    );

    let mut init = command(&runtime, &config, &home, &compat)
        .args([
            "init",
            "PROTONHAX_PREFIX=from-init",
            "PROTONHAX_TEST_TOKEN=hidden",
            proton.to_str().unwrap(),
            "sleep",
            "10",
        ])
        .env("REMOVED", "present")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_context(&runtime, &config, &home, &compat);
    verify_environment_commands(&runtime, &config, &home, &compat);
    verify_detached_run(&directory.0, &runtime, &config, &home, &compat, &helper);
    verify_profiles(&runtime, &config, &home, &compat);
    verify_doctor_fix(&runtime, &config, &home, &compat);

    stop_context(&runtime, &config, &home, &compat, &mut init);
}

fn verify_environment_commands(runtime: &Path, config: &Path, home: &Path, compat: &Path) {
    let environment = successful(command(runtime, config, home, compat).args(["env", "latest"]));
    let environment = String::from_utf8(environment.stdout).unwrap();
    assert!(environment.contains("PROTONHAX_PREFIX=from-init"));
    assert!(!environment.contains("PROTONHAX_TEST_TOKEN"));

    successful(command(runtime, config, home, compat).args([
        "exec",
        "--env",
        "EXEC_VALUE=override",
        "--unset-env",
        "REMOVED",
        "latest",
        "sh",
        "-c",
        "test \"$EXEC_VALUE\" = override && test -z \"${REMOVED:-}\"",
    ]));

    let info =
        successful(command(runtime, config, home, compat).args(["info", "latest", "--json"]));
    let info: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(info["appid"], "570");
    assert_eq!(info["active"], true);
}

fn verify_detached_run(
    directory: &Path,
    runtime: &Path,
    config: &Path,
    home: &Path,
    compat: &Path,
    helper: &Path,
) {
    let output = directory.join("detached.out");
    successful(command(runtime, config, home, compat).args([
        "run",
        "--detach",
        "--cwd",
        "exe-dir",
        "--env",
        "RUN_MARKER=detached",
        "--env",
        &format!("RUN_OUTPUT={}", output.display()),
        "--unset-env",
        "REMOVED",
        "latest",
        helper.to_str().unwrap(),
    ]));

    wait_until(|| output.exists());
    let lines: Vec<_> = fs::read_to_string(output)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        lines,
        ["detached", directory.to_string_lossy().as_ref(), "unset"]
    );
}

fn verify_profiles(runtime: &Path, config: &Path, home: &Path, compat: &Path) {
    successful(command(runtime, config, home, compat).args([
        "profile",
        "add",
        "env-check",
        "latest",
        "--kind",
        "native",
        "--env",
        "PROFILE_VALUE=works",
        "--env",
        "PROFILE_TOKEN=secret",
        "--",
        "sh",
        "-c",
        "test \"$PROFILE_VALUE\" = works",
    ]));
    successful(command(runtime, config, home, compat).args(["profile", "run", "env-check"]));

    let profiles =
        successful(command(runtime, config, home, compat).args(["profile", "ls", "--json"]));
    let profiles = String::from_utf8(profiles.stdout).unwrap();
    assert!(profiles.contains("PROFILE_TOKEN=<redacted>"));
    assert!(!profiles.contains("PROFILE_TOKEN=secret"));

    successful(command(runtime, config, home, compat).args(["profile", "remove", "env-check"]));
    let metadata = fs::metadata(config.join("protonhax/profiles.json")).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

fn verify_doctor_fix(runtime: &Path, config: &Path, home: &Path, compat: &Path) {
    let stale = runtime.join("protonhax/570/4294967295-1");
    fs::create_dir(&stale).unwrap();
    let doctor =
        successful(command(runtime, config, home, compat).args(["doctor", "--json", "--fix"]));
    let doctor: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(doctor["fixes"]["stale_contexts_removed"], 1);
    assert!(!stale.exists());
    assert!(doctor["checks"].as_array().unwrap().iter().any(|check| {
        check["message"]
            .as_str()
            .is_some_and(|message| message.contains("Flatpak Steam detected"))
    }));
}

fn stop_context(runtime: &Path, config: &Path, home: &Path, compat: &Path, init: &mut Child) {
    let info =
        successful(command(runtime, config, home, compat).args(["info", "latest", "--json"]));
    let info: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    let pid = info["pid"].as_u64().unwrap().to_string();
    successful(Command::new("kill").args(["-TERM", &pid]));
    assert!(init.wait().unwrap().code().is_some());
    assert_eq!(
        successful(command(runtime, config, home, compat).arg("ls")).stdout,
        Vec::<u8>::new()
    );
}

fn wait_for_context(runtime: &Path, config: &Path, home: &Path, compat: &Path) {
    wait_until(|| {
        command(runtime, config, home, compat)
            .args(["ls", "--json"])
            .output()
            .is_ok_and(|output| {
                output.status.success() && output.stdout.windows(3).any(|w| w == b"570")
            })
    });
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "timed out waiting for CLI state");
        thread::sleep(Duration::from_millis(20));
    }
}

fn command(runtime: &Path, config: &Path, home: &Path, compat: &Path) -> Command {
    let mut command = Command::new(BIN);
    command
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_CONFIG_HOME", config)
        .env("HOME", home)
        .env("SteamAppId", "570")
        .env("STEAM_COMPAT_DATA_PATH", compat);
    command
}

fn successful(command: &mut Command) -> Output {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
