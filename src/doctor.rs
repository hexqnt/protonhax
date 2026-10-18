use std::{
    env, fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process,
    str::FromStr,
};

use colored::Colorize;
use serde::Serialize;
use serde_json::json;

use crate::{
    app_id::AppId,
    context::{
        ContextDetail, EXE_FILE, PFX_FILE, RepairSummary, RunningApp, all_contexts,
        read_started_at, read_stored_path, repair_contexts,
    },
    env_store::{ENV_FILE, StoredEnv},
    flatpak::{FlatpakStatus, detect as detect_flatpak},
    runtime::format_duration_ago,
    steam::{STEAM_APP_ID_ENV, STEAM_COMPAT_DATA_PATH_ENV, resolve_app_meta},
};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum Level {
    Ok,
    Info,
    Warning,
    Error,
}

#[derive(Serialize)]
struct Check {
    section: String,
    #[serde(rename = "status")]
    level: Level,
    message: String,
}

struct Report {
    section: String,
    checks: Option<Vec<Check>>,
    warnings: usize,
    errors: usize,
}

impl Report {
    fn new(json: bool) -> Self {
        Self {
            section: String::new(),
            checks: json.then(Vec::new),
            warnings: 0,
            errors: 0,
        }
    }

    fn section(&mut self, name: impl Into<String>) {
        let name = name.into();
        if self.checks.is_some() {
            self.section = name;
        } else {
            println!("\n{name}:");
        }
    }

    fn context(&mut self, title: &str) {
        if self.checks.is_some() {
            self.section.clear();
            self.section.push_str("Contexts/");
            self.section.push_str(title);
        } else {
            println!("  {} {title}", "•".cyan().bold());
        }
    }

    fn ok(&mut self, message: impl Into<String>) {
        self.push(Level::Ok, message.into());
    }

    fn info(&mut self, message: impl Into<String>) {
        self.push(Level::Info, message.into());
    }

    fn warn(&mut self, message: impl Into<String>) {
        self.push(Level::Warning, message.into());
    }

    fn error(&mut self, message: impl Into<String>) {
        self.push(Level::Error, message.into());
    }

    fn push(&mut self, level: Level, message: String) {
        match level {
            Level::Warning => self.warnings += 1,
            Level::Error => self.errors += 1,
            Level::Ok | Level::Info => {}
        }
        if let Some(checks) = &mut self.checks {
            checks.push(Check {
                section: self.section.clone(),
                level,
                message,
            });
        } else {
            let label = match level {
                Level::Ok => "OK".green().bold(),
                Level::Info => "INFO".cyan().bold(),
                Level::Warning => "WARN".yellow().bold(),
                Level::Error => "ERR".red().bold(),
            };
            println!("    {label} {message}");
        }
    }

    fn finish(&self, repair: Option<RepairSummary>) -> io::Result<()> {
        if let Some(checks) = &self.checks {
            let data = json!({
                "checks": checks,
                "fixes": repair,
                "summary": {
                    "warnings": self.warnings,
                    "errors": self.errors,
                },
            });
            crate::output::print_json(&data)?;
        } else {
            println!(
                "\nSummary: {} warning(s), {} error(s)",
                self.warnings.to_string().yellow(),
                self.errors.to_string().red()
            );
        }
        Ok(())
    }
}

pub fn handle_doctor(runtime_root: &Path, json_output: bool, fix: bool) -> io::Result<()> {
    let repair = fix.then(|| repair_contexts(runtime_root)).transpose()?;
    let mut report = Report::new(json_output);

    if !json_output {
        println!("{}", "protonhax doctor".bold());
    }
    if let Some(summary) = repair {
        report.section("Fixes");
        report.ok(format!(
            "removed {} stale context(s)",
            summary.stale_contexts_removed
        ));
        report.ok(format!(
            "corrected {} permission set(s)",
            summary.permissions_fixed
        ));
    }

    inspect_environment(&mut report);
    inspect_flatpak(&mut report);
    inspect_runtime(runtime_root, &mut report);
    inspect_contexts(runtime_root, &mut report)?;
    report.finish(repair)?;

    if report.errors > 0 {
        process::exit(1);
    }
    Ok(())
}

fn inspect_flatpak(report: &mut Report) {
    report.section("Flatpak");
    match detect_flatpak() {
        FlatpakStatus::InsideSteam => report.ok(
            "running inside the com.valvesoftware.Steam sandbox; the shared user runtime is available",
        ),
        FlatpakStatus::InsideOther(app_id) => report.warn(format!(
            "running inside an unrelated Flatpak sandbox: {app_id}"
        )),
        FlatpakStatus::InsideUnknown => {
            report.info("running inside a Flatpak sandbox with an unknown application id");
        }
        FlatpakStatus::HostInstallation(path) => {
            report.info(format!("Flatpak Steam detected at {}", path.display()));
            report.info("place protonhax in a home path visible to the Steam sandbox; use `flatpak-spawn --host` only for helpers that must run on the host");
        }
        FlatpakStatus::NotDetected => report.info("Flatpak Steam was not detected"),
    }
}

fn inspect_environment(report: &mut Report) {
    report.section("Environment");
    match env::var(STEAM_APP_ID_ENV) {
        Ok(value) => match AppId::from_str(&value) {
            Ok(appid) => report.ok(format!("{STEAM_APP_ID_ENV}={appid}")),
            Err(error) => report.error(format!("invalid {STEAM_APP_ID_ENV}: {error}")),
        },
        Err(_) => report.info("SteamAppId is not set (expected outside a Steam launch)"),
    }

    match env::var_os(STEAM_COMPAT_DATA_PATH_ENV).map(PathBuf::from) {
        Some(path) if path.is_absolute() && path.exists() => {
            report.ok(format!("{STEAM_COMPAT_DATA_PATH_ENV}={}", path.display()));
        }
        Some(path) => report.warn(format!(
            "{STEAM_COMPAT_DATA_PATH_ENV} is invalid or missing: {}",
            path.display()
        )),
        None => report.info("STEAM_COMPAT_DATA_PATH is not set (expected outside a game launch)"),
    }
}

fn inspect_runtime(runtime_root: &Path, report: &mut Report) {
    report.section("Runtime");
    if runtime_root.exists() {
        report.ok(format!("runtime root: {}", runtime_root.display()));
        inspect_permissions(runtime_root, 0o700, report);
    } else {
        report.warn(format!(
            "runtime root does not exist: {} (no active contexts have been created yet)",
            runtime_root.display()
        ));
    }
}

fn inspect_contexts(runtime_root: &Path, report: &mut Report) -> io::Result<()> {
    report.section("Contexts");
    let contexts = all_contexts(runtime_root, ContextDetail::Identity)?;
    if contexts.is_empty() {
        report.warn("no contexts found");
        return Ok(());
    }
    if !contexts.iter().any(|context| context.active) {
        report.warn(
            "no active contexts found; run `protonhax doctor --fix` to remove stale sessions",
        );
    }
    for app in &contexts {
        inspect_context(app, report);
    }
    Ok(())
}

fn inspect_context(app: &RunningApp, report: &mut Report) {
    let environment = StoredEnv::load(&app.path);
    let name = environment
        .as_ref()
        .ok()
        .and_then(|environment| resolve_app_meta(environment, app.appid).name);
    let title = name.as_deref().map_or_else(
        || format!("{}, session {}", app.appid, app.session_id),
        |name| format!("{} ({name}), session {}", app.appid, app.session_id),
    );
    report.context(&title);

    if app.active {
        report.ok(format!("owner pid {} is active", app.session_id.pid()));
    } else {
        report.warn(format!("owner pid {} is stale", app.session_id.pid()));
    }

    inspect_permissions(&app.path, 0o700, report);
    inspect_file_path(&app.path.join(EXE_FILE), "exe", Level::Error, report);
    inspect_file_path(&app.path.join(PFX_FILE), "pfx", Level::Warning, report);
    inspect_environment_file(app, environment, report);

    if let Some(started_at) = read_started_at(&app.path) {
        report.ok(format!(
            "started_at: {started_at} ({})",
            format_duration_ago(started_at)
        ));
    } else {
        report.warn("started_at is missing or invalid");
    }
}

fn inspect_environment_file(
    app: &RunningApp,
    environment: io::Result<StoredEnv>,
    report: &mut Report,
) {
    match environment {
        Ok(environment) => {
            report.ok("env: environment file is readable");
            inspect_permissions(&app.path.join(ENV_FILE), 0o600, report);
            match environment.get(STEAM_COMPAT_DATA_PATH_ENV) {
                Some(path) if Path::new(path).exists() => report.ok(format!(
                    "env.STEAM_COMPAT_DATA_PATH: {}",
                    Path::new(path).display()
                )),
                Some(path) => report.warn(format!(
                    "env.STEAM_COMPAT_DATA_PATH points to a missing path: {}",
                    Path::new(path).display()
                )),
                None => report.warn("env: STEAM_COMPAT_DATA_PATH is missing"),
            }
        }
        Err(error) => report.error(format!("env file is missing or invalid: {error}")),
    }
}

fn inspect_file_path(stored_path: &Path, label: &str, severity: Level, report: &mut Report) {
    let problem = match read_stored_path(stored_path) {
        Ok(path) if path.exists() => {
            report.ok(format!("{label}: {}", path.display()));
            return;
        }
        Ok(path) => format!("{label} path does not exist: {}", path.display()),
        Err(error) => format!("{label} file is missing or unreadable: {error}"),
    };

    report.push(severity, problem);
}

fn inspect_permissions(path: &Path, expected: u32, report: &mut Report) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let actual = metadata.permissions().mode() & 0o777;
    if actual == expected {
        report.ok(format!("permissions for {}: {actual:o}", path.display()));
    } else {
        report.warn(format!(
            "permissions for {} are {actual:o}, expected {expected:o}",
            path.display()
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, process};

    use crate::{context::SessionId, test_support::TestDirectory};

    use super::{Report, inspect_contexts};

    #[test]
    fn report_records_sections_and_counts_levels() {
        let mut report = Report::new(true);
        report.section("Test");
        report.ok("ok");
        report.info("info");
        report.warn("warning");
        report.error("error");

        assert_eq!((report.warnings, report.errors), (1, 1));
        let checks = serde_json::to_value(report.checks.unwrap()).unwrap();
        let checks = checks.as_array().unwrap();
        assert!(checks.iter().all(|check| check["section"] == "Test"));
        assert_eq!(
            checks
                .iter()
                .map(|check| check["status"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["ok", "info", "warning", "error"]
        );
    }

    #[test]
    fn incomplete_context_preserves_diagnostic_messages_and_severity() {
        let directory = TestDirectory::new().unwrap();
        let root = directory.path();
        let session = SessionId::for_pid(process::id()).unwrap();
        let context = root.join("570").join(session.to_string());
        fs::create_dir_all(&context).unwrap();
        fs::set_permissions(&context, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(context.join("started_at"), "invalid").unwrap();
        let mut report = Report::new(true);
        inspect_contexts(root, &mut report).unwrap();

        assert_eq!((report.warnings, report.errors), (2, 2));
        let checks = serde_json::to_value(report.checks.unwrap()).unwrap();
        let checks = checks.as_array().unwrap();
        assert!(
            checks
                .iter()
                .all(|check| check["section"] == format!("Contexts/570, session {session}"))
        );
        for (prefix, status) in [
            ("exe file is missing or unreadable:", "error"),
            ("pfx file is missing or unreadable:", "warning"),
            ("env file is missing or invalid:", "error"),
            ("started_at is missing or invalid", "warning"),
        ] {
            assert!(checks.iter().any(|check| {
                check["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with(prefix))
                    && check["status"] == status
            }));
        }
    }
}
