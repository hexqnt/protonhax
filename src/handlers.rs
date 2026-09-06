use std::{
    env,
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
    context::{
        ContextDetail, EXE_FILE, PFX_FILE, PendingContext, RunningApp, active_apps,
        read_stored_path,
    },
    env_store::StoredEnv,
    launch::{EnvironmentChanges, RunOptions, configure_detached},
    profile::{self, ProfileKind, ProfileName},
    proton::ProtonInvocation,
    runtime::{format_duration_ago, unix_now_secs},
    shell::{is_env_assignment, split_env_assignment},
    steam::{STEAM_APP_ID_ENV, STEAM_COMPAT_DATA_PATH_ENV},
};

struct TargetApp {
    appid: AppId,
    context_dir: PathBuf,
}

struct RunContext {
    target: TargetApp,
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
        command.env(STEAM_APP_ID_ENV, self.target.appid.to_string());
        changes.apply_to(&mut command);
        command
    }
}

struct InitCommand {
    tokens: Vec<String>,
    cmd_start_index: usize,
}

impl InitCommand {
    fn command(&self) -> &[String] {
        &self.tokens[self.cmd_start_index..]
    }

    fn env_assignments(&self) -> &[String] {
        &self.tokens[..self.cmd_start_index]
    }
}

pub fn handle_init(runtime_root: &Path, cmd: Vec<String>, debug: bool) -> io::Result<()> {
    if cmd.is_empty() {
        print_subcommand_usage_error("init", "No command specified");
    }

    let appid = required_app_id();
    let init_command = parse_init_command(cmd);
    let real_cmd = init_command.command();
    let search_path = init_command
        .env_assignments()
        .iter()
        .find_map(|assignment| {
            split_env_assignment(assignment)
                .and_then(|(name, value)| (name == "PATH").then_some(std::ffi::OsStr::new(value)))
        });
    let proton = ProtonInvocation::parse(real_cmd, search_path)
        .unwrap_or_else(|error| print_subcommand_usage_error("init", &error.to_string()));
    let compat_data = required_absolute_env_path(STEAM_COMPAT_DATA_PATH_ENV);
    let prefix_path = compat_data.join("pfx");
    let environment = StoredEnv::capture(init_command.env_assignments());
    let pending = PendingContext::create(
        runtime_root,
        appid,
        proton.executable(),
        &prefix_path,
        &environment,
        unix_now_secs(),
    )?;

    let mut command = process::Command::new(&real_cmd[0]);
    command.args(&real_cmd[1..]);
    for assignment in init_command.env_assignments() {
        if let Some((name, value)) = split_env_assignment(assignment) {
            command.env(name, value);
        }
    }

    if debug {
        eprintln!(
            "{} Executing command (argv): {real_cmd:?}",
            "DEBUG".bold().cyan()
        );
    }

    let mut child = command.spawn()?;
    let published = match pending.publish(child.id()) {
        Ok(context) => context,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let status = child.wait();
    drop(published);
    exit_with_status(status?);
}

pub fn handle_ls(runtime_root: &Path, long: bool, json_output: bool) -> io::Result<()> {
    let detail = if long || json_output {
        ContextDetail::Full
    } else {
        ContextDetail::Summary
    };
    let apps = active_apps(runtime_root, detail)?;

    if json_output {
        return print_ls_json(&apps);
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
    let exe = read_stored_path(context.target.context_dir.join(EXE_FILE))?;
    let mut command = context.command(exe, &options.environment);
    command.arg("run").args(cmd);
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
    let exe = read_stored_path(context.target.context_dir.join(EXE_FILE))?;
    let pfx = read_stored_path(context.target.context_dir.join(PFX_FILE))?;
    let cmd_exe = pfx.join("drive_c/windows/system32/cmd.exe");

    let status = context
        .command(exe, &EnvironmentChanges::default())
        .arg("run")
        .arg(cmd_exe)
        .status()?;
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
    let app = resolve_running_app(runtime_root, selector, ContextDetail::Identity)?;
    let environment = StoredEnv::load(&app.path)?;
    let stdout = io::stdout();
    environment.write_display(stdout.lock())
}

pub fn handle_info(
    runtime_root: &Path,
    selector: &TargetSelector,
    json_output: bool,
) -> io::Result<()> {
    let app = resolve_running_app(runtime_root, selector, ContextDetail::Full)?;
    if json_output {
        let stdout = io::stdout();
        let mut output = stdout.lock();
        serde_json::to_writer_pretty(&mut output, &app_json(&app)).map_err(io::Error::other)?;
        return writeln!(output);
    }

    let stdout = io::stdout();
    let mut output = stdout.lock();
    print_app_header(&mut output, &app, true)?;
    writeln!(output, "  {} {}", "Session:".dimmed(), app.session_id)?;
    writeln!(output, "  {} {}", "PID:".dimmed(), app.session_id.pid())?;
    print_app_paths(&mut output, &app)
}

fn parse_init_command(cmd: Vec<String>) -> InitCommand {
    // Steam иногда прокидывает %COMMAND% одной shell-строкой.
    let tokens = if let [command] = cmd.as_slice()
        && command.contains(char::is_whitespace)
    {
        shell_words::split(command).unwrap_or_else(|error| {
            print_subcommand_usage_error("init", &format!("Failed to parse command: {error}"));
        })
    } else {
        cmd
    };

    let Some(cmd_start_index) = tokens
        .iter()
        .position(|argument| !is_env_assignment(argument))
    else {
        print_subcommand_usage_error("init", "No command specified after environment assignments");
    };

    InitCommand {
        tokens,
        cmd_start_index,
    }
}

fn prepare_context(runtime_root: &Path, selector: &TargetSelector) -> io::Result<RunContext> {
    let app = resolve_running_app(runtime_root, selector, ContextDetail::Identity)?;
    let env = StoredEnv::load(&app.path)?;
    let target = to_target_app(app);
    Ok(RunContext { target, env })
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

    Ok(index.map_or_else(
        || {
            eprintln!(
                "{} No active context matches the requested target. Start a game through Steam first.",
                "Error:".bold().red()
            );
            process::exit(2);
        },
        |index| apps.swap_remove(index),
    ))
}

fn resolve_app_by_name(apps: &[RunningApp], query: &str) -> usize {
    let mut matches = apps.iter().enumerate().filter(|app| {
        app.1
            .name
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

fn to_target_app(app: RunningApp) -> TargetApp {
    TargetApp {
        appid: app.appid,
        context_dir: app.path,
    }
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

fn print_ls_json(apps: &[RunningApp]) -> io::Result<()> {
    let data: Vec<_> = apps.iter().map(app_json).collect();

    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, &data).map_err(io::Error::other)?;
    writeln!(output)
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
    if let Some(prefix_path) = &app.prefix_path {
        writeln!(output, "  {} {}", "Prefix:".dimmed(), prefix_path.display())?;
    }
    if let Some(install_path) = &app.install_path {
        writeln!(
            output,
            "  {} {}",
            "Install:".dimmed(),
            install_path.display()
        )?;
    }
    if let Some(proton_path) = &app.proton_path {
        writeln!(output, "  {} {}", "Proton:".dimmed(), proton_path.display())?;
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

fn required_app_id() -> AppId {
    let value = env::var(STEAM_APP_ID_ENV).unwrap_or_else(|_| {
        print_subcommand_usage_error("init", &format!("{STEAM_APP_ID_ENV} is not set"));
    });
    value.parse().unwrap_or_else(|error| {
        print_subcommand_usage_error("init", &format!("Invalid {STEAM_APP_ID_ENV}: {error}"));
    })
}

fn required_absolute_env_path(name: &str) -> PathBuf {
    let path = env::var_os(name).map_or_else(
        || print_subcommand_usage_error("init", &format!("{name} is not set")),
        PathBuf::from,
    );
    if !path.is_absolute() {
        print_subcommand_usage_error("init", &format!("{name} must be an absolute path"));
    }
    path
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
    use std::process::Command;

    use super::{contains_case_insensitive, status_code};

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
