use std::{
    io::{self, BufWriter, Write},
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process,
};

use colored::Colorize;
use serde_json::json;

use crate::{
    app_id::{AppId, TargetSelector},
    cli::sub_usage,
    context::{ContextDetail, EXE_FILE, PFX_FILE, RunningApp, active_apps, read_stored_path},
    env_store::StoredEnv,
    launch::{EnvironmentChanges, RunOptions, configure_detached},
    output::print_json,
    profile::{self, ProfileKind, ProfileName},
    runtime::format_duration_ago,
    steam::STEAM_APP_ID_ENV,
};

pub use init::handle_init;

mod init;

struct RunContext {
    appid: AppId,
    context_dir: PathBuf,
    env: StoredEnv,
}

impl RunContext {
    fn command(
        &self,
        program: impl AsRef<std::ffi::OsStr>,
        changes: &EnvironmentChanges,
    ) -> process::Command {
        let mut command = process::Command::new(program);
        self.env.configure(&mut command);
        command.env(STEAM_APP_ID_ENV, self.appid.to_string());
        changes.apply_to(&mut command);
        command
    }

    fn proton_command(&self, changes: &EnvironmentChanges) -> io::Result<process::Command> {
        let executable = read_stored_path(self.context_dir.join(EXE_FILE))?;
        let mut command = self.command(executable, changes);
        command.arg("run");
        Ok(command)
    }
}

pub fn handle_ls(runtime_root: &Path, long: bool, json_output: bool) -> io::Result<()> {
    let detail = if long || json_output {
        ContextDetail::Full
    } else {
        ContextDetail::Summary
    };
    let apps = active_apps(runtime_root, detail)?;

    if json_output {
        return print_json(&apps.iter().map(app_json).collect::<Vec<_>>());
    }

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    for app in &apps {
        print_app_header(&mut output, app, long)?;
        if long {
            print_app_paths(&mut output, app)?;
        }
    }

    Ok(())
}

pub fn handle_run(
    runtime_root: &Path,
    selector: &TargetSelector,
    cmd: &[String],
    options: &RunOptions,
) -> io::Result<()> {
    let Some((executable, _)) = cmd.split_first() else {
        print_subcommand_usage_error("run", "No command specified");
    };

    let context = prepare_context(runtime_root, selector)?;
    let mut command = context.proton_command(&options.environment)?;
    command.args(cmd);
    if let Some(cwd) = &options.cwd {
        cwd.apply_to(&mut command, executable);
    }
    if options.detach {
        configure_detached(&mut command);
        command.spawn()?;
        return Ok(());
    }

    let status = command.status()?;
    exit_with_status(status);
}

pub fn handle_cmd(runtime_root: &Path, selector: &TargetSelector) -> io::Result<()> {
    let context = prepare_context(runtime_root, selector)?;
    let mut command = context.proton_command(&EnvironmentChanges::default())?;
    let pfx = read_stored_path(context.context_dir.join(PFX_FILE))?;
    let cmd_exe = pfx.join("drive_c/windows/system32/cmd.exe");

    let status = command.arg(cmd_exe).status()?;
    exit_with_status(status);
}

pub fn handle_exec(
    runtime_root: &Path,
    selector: &TargetSelector,
    cmd: &[String],
    environment: &EnvironmentChanges,
) -> io::Result<()> {
    let Some((executable, args)) = cmd.split_first() else {
        print_subcommand_usage_error("exec", "No command specified");
    };

    let context = prepare_context(runtime_root, selector)?;
    let status = context
        .command(executable, environment)
        .args(args)
        .status()?;
    exit_with_status(status);
}

pub fn handle_profile_run(
    runtime_root: &Path,
    name: &ProfileName,
    extra_args: &[String],
) -> io::Result<()> {
    let profile = profile::resolve(name, extra_args)?;
    match profile.kind {
        ProfileKind::Proton => handle_run(
            runtime_root,
            &profile.target,
            &profile.command,
            &profile.run_options,
        ),
        ProfileKind::Native => handle_exec(
            runtime_root,
            &profile.target,
            &profile.command,
            &profile.run_options.environment,
        ),
    }
}

pub fn handle_env(runtime_root: &Path, selector: &TargetSelector) -> io::Result<()> {
    let context = prepare_context(runtime_root, selector)?;
    let stdout = io::stdout();
    context.env.write_display(stdout.lock())
}

pub fn handle_info(
    runtime_root: &Path,
    selector: &TargetSelector,
    json_output: bool,
) -> io::Result<()> {
    let app = resolve_running_app(runtime_root, selector, ContextDetail::Full)?;
    if json_output {
        return print_json(&app_json(&app));
    }

    let stdout = io::stdout();
    let mut output = stdout.lock();
    print_app_header(&mut output, &app, true)?;
    writeln!(output, "  {} {}", "Session:".dimmed(), app.session_id)?;
    writeln!(output, "  {} {}", "PID:".dimmed(), app.session_id.pid())?;
    print_app_paths(&mut output, &app)
}

fn prepare_context(runtime_root: &Path, selector: &TargetSelector) -> io::Result<RunContext> {
    let app = resolve_running_app(runtime_root, selector, ContextDetail::Identity)?;
    let env = StoredEnv::load(&app.path)?;
    Ok(RunContext {
        appid: app.appid,
        context_dir: app.path,
        env,
    })
}

fn resolve_running_app(
    runtime_root: &Path,
    selector: &TargetSelector,
    detail: ContextDetail,
) -> io::Result<RunningApp> {
    let detail = match selector {
        TargetSelector::Name(_) => detail.max(ContextDetail::Summary),
        TargetSelector::Latest | TargetSelector::AppId(_) => detail,
    };
    let mut apps = active_apps(runtime_root, detail)?;
    let index = match selector {
        TargetSelector::Latest => apps
            .iter()
            .enumerate()
            .max_by_key(|(_, app)| app.recency())
            .map(|(index, _)| index),
        TargetSelector::AppId(appid) => apps.iter().position(|app| app.appid == *appid),
        TargetSelector::Name(query) => Some(resolve_app_by_name(&apps, query)),
    };

    let Some(index) = index else {
        eprintln!(
            "{} No active context matches the requested target. Start a game through Steam first.",
            "Error:".bold().red()
        );
        process::exit(2);
    };
    Ok(apps.swap_remove(index))
}

fn resolve_app_by_name(apps: &[RunningApp], query: &str) -> usize {
    let mut matches = apps.iter().enumerate().filter(|(_, app)| {
        app.name
            .as_deref()
            .is_some_and(|name| contains_case_insensitive(name, query))
    });

    let Some((index, first)) = matches.next() else {
        eprintln!(
            "{} No running application has a matching name \"{query}\".",
            "Error:".bold().red()
        );
        process::exit(2);
    };
    let Some((_, second)) = matches.next() else {
        return index;
    };

    print_ambiguous_matches(
        query,
        [first, second]
            .into_iter()
            .chain(matches.map(|(_, app)| app)),
    );
    process::exit(2);
}

fn print_ambiguous_matches<'a>(query: &str, matches: impl IntoIterator<Item = &'a RunningApp>) {
    eprintln!(
        "{} Multiple applications match name \"{query}\":",
        "Error:".bold().red()
    );
    for app in matches {
        let name = app.name.as_deref().unwrap_or("<unnamed>");
        eprintln!("  {}  {}", app.appid.to_string().green(), name.yellow());
    }
    eprintln!("Specify an appid from `protonhax ls -l`.");
}

fn app_json(app: &RunningApp) -> serde_json::Value {
    json!({
        "appid": app.appid.to_string(),
        "session": app.session_id.to_string(),
        "pid": app.session_id.pid(),
        "active": app.active,
        "name": app.name,
        "install_path": app.install_path.as_ref().map(|path| path.to_string_lossy()),
        "compatdata_path": app.compatdata_path.as_ref().map(|path| path.to_string_lossy()),
        "prefix_path": app.prefix_path.as_ref().map(|path| path.to_string_lossy()),
        "proton_path": app.proton_path.as_ref().map(|path| path.to_string_lossy()),
        "started_at": app.started_at,
        "started_ago": app.started_at.map(format_duration_ago),
    })
}

fn print_app_header(
    output: &mut impl Write,
    app: &RunningApp,
    show_timing: bool,
) -> io::Result<()> {
    write!(output, "{}", app.appid.to_string().green())?;
    if let Some(name) = &app.name {
        write!(output, "  {}", name.yellow())?;
    }
    if show_timing && let Some(started_at) = app.started_at {
        write!(
            output,
            "  {}",
            format!("started {}", format_duration_ago(started_at)).dimmed()
        )?;
    }
    writeln!(output)
}

fn print_app_paths(output: &mut impl Write, app: &RunningApp) -> io::Result<()> {
    for (label, path) in [
        ("Prefix:", &app.prefix_path),
        ("Install:", &app.install_path),
        ("Proton:", &app.proton_path),
    ] {
        if let Some(path) = path {
            writeln!(output, "  {} {}", label.dimmed(), path.display())?;
        }
    }
    Ok(())
}

fn contains_case_insensitive(text: &str, query: &str) -> bool {
    if text.is_ascii() && query.is_ascii() {
        return contains_ascii_case_insensitive(text.as_bytes(), query.as_bytes());
    }

    text.to_lowercase().contains(&query.to_lowercase())
}

fn contains_ascii_case_insensitive(text: &[u8], query: &[u8]) -> bool {
    query.is_empty()
        || text
            .windows(query.len())
            .any(|window| window.eq_ignore_ascii_case(query))
}

fn print_subcommand_usage_error(subcommand: &str, message: &str) -> ! {
    eprintln!("{} {message}", "Error:".bold().red());
    sub_usage(subcommand);
    process::exit(1);
}

fn exit_with_status(status: process::ExitStatus) -> ! {
    process::exit(status_code(status));
}

fn status_code(status: process::ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        fs,
        os::unix::ffi::{OsStrExt, OsStringExt},
        path::PathBuf,
        process::Command,
    };

    use crate::{
        context::EXE_FILE, env_store::StoredEnv, launch::EnvironmentChanges,
        steam::STEAM_APP_ID_ENV, test_support::TestDirectory,
    };

    use super::{RunContext, contains_case_insensitive, status_code};

    #[test]
    fn context_applies_appid_before_user_changes_and_removals() {
        let context = RunContext {
            appid: "570".parse().unwrap(),
            context_dir: PathBuf::new(),
            env: StoredEnv::capture(&[
                "SteamAppId=1".to_owned(),
                "PROTONHAX_VALUE=stored".to_owned(),
            ]),
        };
        let command = context.command("native", &EnvironmentChanges::default());
        assert!(
            command
                .get_envs()
                .any(|entry| entry == (OsStr::new(STEAM_APP_ID_ENV), Some(OsStr::new("570"))))
        );

        let changes = EnvironmentChanges {
            overrides: vec![
                "SteamAppId=2".parse().unwrap(),
                "PROTONHAX_VALUE=override".parse().unwrap(),
            ],
            removals: vec!["SteamAppId".parse().unwrap()],
        };
        let command = context.command("native", &changes);
        assert_eq!(command.get_program(), "native");
        assert_eq!(command.get_args().count(), 0);
        assert!(command.get_envs().all(|(name, _)| name != STEAM_APP_ID_ENV));
        assert!(command.get_envs().any(|entry| {
            entry == (OsStr::new("PROTONHAX_VALUE"), Some(OsStr::new("override")))
        }));
    }

    #[test]
    fn proton_command_preserves_non_utf8_executable_and_run_verb() {
        let directory = TestDirectory::new().unwrap();
        let executable = OsString::from_vec(b"/tools/\xff/proton".to_vec());
        fs::write(directory.path().join(EXE_FILE), executable.as_bytes()).unwrap();
        let context = RunContext {
            appid: "570".parse().unwrap(),
            context_dir: directory.path().to_owned(),
            env: StoredEnv::capture(&[]),
        };
        let command = context
            .proton_command(&EnvironmentChanges::default())
            .unwrap();
        assert_eq!(command.get_program(), &executable);
        assert!(command.get_args().eq([OsStr::new("run")]));
    }

    #[test]
    fn case_insensitive_search() {
        assert!(contains_case_insensitive("Gunfire Reborn", "gunfire"));
        assert!(contains_case_insensitive("GUNFIRE REBORN", "reborn"));
        assert!(!contains_case_insensitive("Gunfire Reborn", "helldivers"));
    }

    #[test]
    fn child_signal_uses_shell_compatible_exit_code() {
        let status = Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .unwrap();
        assert_eq!(status_code(status), 143);
    }
}
