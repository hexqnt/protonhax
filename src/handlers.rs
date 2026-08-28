use std::{
    env, fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    process,
};

use colored::Colorize;
use serde_json::json;

use crate::{
    cli::sub_usage,
    env_store::{ENV_FILE, StoredEnv, get_env_var},
    runtime::{format_duration_ago, unix_now_secs},
    shell::{is_env_assignment, shell_escape, split_env_assignment},
    steam::{AppMeta, resolve_app_meta},
};

const EXE_FILE: &str = "exe";
const PFX_FILE: &str = "pfx";
const STARTED_AT_FILE: &str = "started_at";
const STEAM_APP_ID_ENV: &str = "SteamAppId";
const STEAM_COMPAT_DATA_PATH_ENV: &str = "STEAM_COMPAT_DATA_PATH";
const LATEST_SELECTOR: &str = "latest";

#[derive(Clone, Copy)]
enum AppData {
    Basic,
    Timing,
    Full,
}

struct RunningApp {
    appid: String,
    path: PathBuf,
    name: Option<String>,
    install_path: Option<PathBuf>,
    compatdata_path: Option<PathBuf>,
    prefix_path: Option<PathBuf>,
    proton_path: Option<PathBuf>,
    started_at: Option<u64>,
}

struct TargetApp {
    appid: String,
    app_dir: PathBuf,
}

struct RunContext {
    target: TargetApp,
    env: StoredEnv,
}

impl RunContext {
    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> process::Command {
        let mut command = process::Command::new(program);
        self.env.apply_to(&mut command);
        command.env(STEAM_APP_ID_ENV, &self.target.appid);
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

pub fn handle_init(phd: &Path, cmd: Vec<String>, debug: bool) -> io::Result<()> {
    if cmd.is_empty() {
        print_subcommand_usage_error("init", "No command specified");
    }

    let appid = required_env_var(STEAM_APP_ID_ENV, "init");
    let app_dir = phd.join(&appid);
    fs::create_dir_all(&app_dir)?;

    // Сохраняем время старта (unix epoch, секунды).
    fs::write(app_dir.join(STARTED_AT_FILE), unix_now_secs().to_string())?;

    let init_command = parse_init_command(cmd);
    let real_cmd = init_command.command();
    // Находим путь к proton в аргументах.
    let Some(proton_path) = real_cmd.iter().find(|arg| arg.contains("/proton")) else {
        print_subcommand_usage_error("init", "Proton path not found in command");
    };

    // Сохраняем данные.
    fs::write(app_dir.join(EXE_FILE), proton_path)?;

    // Сохраняем путь к pfx.
    let compat_data = required_env_var(STEAM_COMPAT_DATA_PATH_ENV, "init");
    fs::write(app_dir.join(PFX_FILE), format!("{compat_data}/pfx"))?;

    // Сохраняем окружение в формате declare -x.
    write_env_file(&app_dir)?;

    // Выполняем исходную команду, учитывая возможные префиксные VAR=VALUE присваивания.
    let mut child = process::Command::new(&real_cmd[0]);
    child.args(&real_cmd[1..]);

    for assign in init_command.env_assignments() {
        let Some((name, value)) = split_env_assignment(assign) else {
            continue;
        };
        child.env(name, value);
    }

    if debug {
        eprintln!(
            "{} Executing command (argv): {real_cmd:?}",
            "DEBUG".bold().cyan()
        );
    }

    let status = child.status()?;

    // Удаляем директорию.
    let _ = fs::remove_dir_all(&app_dir);
    exit_with_status(status);
}

pub fn handle_ls(phd: &Path, long: bool, json_output: bool) -> io::Result<()> {
    let data = if long || json_output {
        AppData::Full
    } else {
        AppData::Basic
    };
    let apps = collect_running_apps(phd, data)?;

    if json_output {
        return print_ls_json(&apps);
    }

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    for app in apps {
        if !long {
            write!(output, "{}", app.appid.green())?;
            if let Some(name) = app.name {
                write!(output, "  {}", name.yellow())?;
            }
            writeln!(output)?;
            continue;
        }

        write!(output, "{}", app.appid.green())?;
        if let Some(name) = app.name {
            write!(output, "  {}", name.yellow())?;
        }
        if let Some(started_at) = app.started_at {
            write!(
                output,
                "  {}",
                format!("started {}", format_duration_ago(started_at)).dimmed()
            )?;
        }
        writeln!(output)?;
        if let Some(prefix_path) = app.prefix_path {
            writeln!(output, "  {} {}", "Prefix:".dimmed(), prefix_path.display())?;
        }
        if let Some(install_path) = app.install_path {
            writeln!(
                output,
                "  {} {}",
                "Install:".dimmed(),
                install_path.display()
            )?;
        }
        if let Some(proton_path) = app.proton_path {
            writeln!(output, "  {} {}", "Proton:".dimmed(), proton_path.display())?;
        }
    }

    Ok(())
}

pub fn handle_run(phd: &Path, appid: &str, cmd: &[String]) -> io::Result<()> {
    if cmd.is_empty() {
        print_subcommand_usage_error("run", "No command specified");
    }

    let context = prepare_context(phd, appid)?;
    let exe = read_trimmed(context.target.app_dir.join(EXE_FILE))?;
    let status = context.command(exe).arg("run").args(cmd).status()?;
    exit_with_status(status);
}

pub fn handle_cmd(phd: &Path, appid: &str) -> io::Result<()> {
    let context = prepare_context(phd, appid)?;
    let exe = read_trimmed(context.target.app_dir.join(EXE_FILE))?;
    let pfx = read_trimmed(context.target.app_dir.join(PFX_FILE))?;
    let cmd_exe = format!("{pfx}/drive_c/windows/system32/cmd.exe");

    let status = context.command(exe).arg("run").arg(cmd_exe).status()?;
    exit_with_status(status);
}

pub fn handle_exec(phd: &Path, appid: &str, cmd: &[String]) -> io::Result<()> {
    if cmd.is_empty() {
        print_subcommand_usage_error("exec", "No command specified");
    }

    let context = prepare_context(phd, appid)?;
    let status = context.command(&cmd[0]).args(&cmd[1..]).status()?;
    exit_with_status(status);
}

pub fn handle_doctor(phd: &Path) -> io::Result<()> {
    let mut warnings = 0usize;
    let mut errors = 0usize;

    println!("{}", "protonhax doctor".bold());

    println!("\nEnvironment:");
    if let Ok(steam_app_id) = env::var(STEAM_APP_ID_ENV) {
        doctor_ok(&format!("{STEAM_APP_ID_ENV}={steam_app_id}"));
    } else {
        doctor_info("SteamAppId is not set (expected outside a Steam launch)");
    }

    match env::var(STEAM_COMPAT_DATA_PATH_ENV) {
        Ok(path) => {
            if Path::new(&path).exists() {
                doctor_ok(&format!("{STEAM_COMPAT_DATA_PATH_ENV}={path}"));
            } else {
                warnings += 1;
                doctor_warn(&format!(
                    "{STEAM_COMPAT_DATA_PATH_ENV} is set, but the path does not exist: {path}"
                ));
            }
        }
        Err(_) => {
            doctor_info("STEAM_COMPAT_DATA_PATH is not set (expected outside a game launch)");
        }
    }

    println!("\nRuntime:");
    if phd.exists() {
        doctor_ok(&format!("runtime root: {}", phd.display()));
    } else {
        warnings += 1;
        doctor_warn(&format!(
            "runtime root does not exist: {} (no active contexts have been created yet)",
            phd.display()
        ));
    }

    println!("\nContexts:");
    let apps = collect_running_apps(phd, AppData::Full)?;
    if apps.is_empty() {
        warnings += 1;
        doctor_warn("no active contexts found");
    }

    for app in &apps {
        inspect_context(app, &mut warnings, &mut errors);
    }

    println!(
        "\nSummary: {} warning(s), {} error(s)",
        warnings.to_string().yellow(),
        errors.to_string().red()
    );

    if errors > 0 {
        process::exit(1);
    }

    Ok(())
}

fn parse_init_command(cmd: Vec<String>) -> InitCommand {
    // Steam иногда прокидывает %COMMAND% одной shell-строкой.
    let tokens = if cmd.len() == 1 && cmd[0].contains(char::is_whitespace) {
        shell_words::split(&cmd[0]).unwrap_or_else(|err| {
            print_subcommand_usage_error("init", &format!("Failed to parse command: {err}"));
        })
    } else {
        cmd
    };

    let Some(cmd_start_index) = tokens.iter().position(|arg| !is_env_assignment(arg)) else {
        print_subcommand_usage_error("init", "No command specified after environment assignments");
    };

    InitCommand {
        tokens,
        cmd_start_index,
    }
}

fn prepare_context(phd: &Path, selector: &str) -> io::Result<RunContext> {
    let target = resolve_target_app(phd, selector)?;
    let env = StoredEnv::load(&target.app_dir)?;
    Ok(RunContext { target, env })
}

fn resolve_target_app(phd: &Path, selector: &str) -> io::Result<TargetApp> {
    if selector.eq_ignore_ascii_case(LATEST_SELECTOR) {
        return resolve_latest_app(phd);
    }

    let app_dir = phd.join(selector);
    if app_dir.is_dir() {
        return Ok(TargetApp {
            appid: selector.to_string(),
            app_dir,
        });
    }

    resolve_app_by_name(phd, selector)
}

fn resolve_latest_app(phd: &Path) -> io::Result<TargetApp> {
    let mut apps = collect_running_apps(phd, AppData::Timing)?;
    if apps.is_empty() {
        eprintln!(
            "{} No active contexts. Start a game through Steam first.",
            "Error:".bold().red()
        );
        process::exit(2);
    }

    if let Some(index) = apps
        .iter()
        .enumerate()
        .filter_map(|(index, app)| app.started_at.map(|started_at| (started_at, index)))
        .max_by_key(|(started_at, _)| *started_at)
        .map(|(_, index)| index)
    {
        let app = apps.swap_remove(index);
        return Ok(TargetApp {
            appid: app.appid,
            app_dir: app.path,
        });
    }

    if apps.len() == 1 {
        let app = apps.pop().expect("length checked above");
        return Ok(TargetApp {
            appid: app.appid,
            app_dir: app.path,
        });
    }

    eprintln!(
        "{} Cannot resolve latest: active contexts have no valid started_at.",
        "Error:".bold().red()
    );
    eprintln!("Specify an appid explicitly (see `protonhax ls -l`).");
    process::exit(2);
}

fn resolve_app_by_name(phd: &Path, query: &str) -> io::Result<TargetApp> {
    let apps = collect_running_apps(phd, AppData::Full)?;
    let matches: Vec<&RunningApp> = apps
        .iter()
        .filter(|app| {
            app.name
                .as_deref()
                .is_some_and(|name| contains_case_insensitive(name, query))
        })
        .collect();

    match matches.as_slice() {
        [app] => Ok(TargetApp {
            appid: app.appid.clone(),
            app_dir: app.path.clone(),
        }),
        [] => {
            eprintln!(
                "{} No running application has appid \"{query}\" or a matching name.",
                "Error:".bold().red()
            );
            process::exit(2);
        }
        _ => {
            print_ambiguous_matches(query, &matches);
            process::exit(2);
        }
    }
}

fn print_ambiguous_matches(query: &str, matches: &[&RunningApp]) {
    eprintln!(
        "{} Multiple applications match name \"{query}\":",
        "Error:".bold().red()
    );
    for app in matches {
        let name = app.name.as_deref().unwrap_or("<unnamed>");
        eprintln!("  {}  {}", app.appid.green(), name.yellow());
    }
    eprintln!("Specify an appid from `protonhax ls -l`.");
}

fn collect_running_apps(phd: &Path, data: AppData) -> io::Result<Vec<RunningApp>> {
    if !phd.exists() {
        return Ok(Vec::new());
    }

    let mut apps = Vec::new();
    for entry in fs::read_dir(phd)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let appid = entry.file_name().to_string_lossy().into_owned();
        let meta = if matches!(data, AppData::Basic | AppData::Full) {
            resolve_app_meta(&path, &appid)
        } else {
            AppMeta::default()
        };
        let started_at = match data {
            AppData::Basic => None,
            AppData::Timing | AppData::Full => read_started_at(&path),
        };
        let (prefix_path, proton_path) = if matches!(data, AppData::Full) {
            (
                read_trimmed(path.join(PFX_FILE)).ok().map(PathBuf::from),
                read_trimmed(path.join(EXE_FILE)).ok().map(PathBuf::from),
            )
        } else {
            (None, None)
        };

        apps.push(RunningApp {
            appid,
            path,
            name: meta.name,
            install_path: meta.install_path,
            compatdata_path: meta.compatdata_path,
            prefix_path,
            proton_path,
            started_at,
        });
    }

    apps.sort_by(|left, right| left.appid.cmp(&right.appid));
    Ok(apps)
}

fn print_ls_json(apps: &[RunningApp]) -> io::Result<()> {
    let data: Vec<_> = apps
        .iter()
        .map(|app| {
            json!({
                "appid": app.appid,
                "name": app.name,
                "install_path": app.install_path.as_ref().map(|path| path.to_string_lossy()),
                "compatdata_path": app.compatdata_path.as_ref().map(|path| path.to_string_lossy()),
                "prefix_path": app.prefix_path.as_ref().map(|path| path.to_string_lossy()),
                "proton_path": app.proton_path.as_ref().map(|path| path.to_string_lossy()),
                "started_at": app.started_at,
                "started_ago": app.started_at.map(format_duration_ago),
            })
        })
        .collect();

    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, &data).map_err(io::Error::other)?;
    writeln!(output)
}

fn inspect_context(app: &RunningApp, warnings: &mut usize, errors: &mut usize) {
    let title = match app.name.as_deref() {
        Some(name) => format!("{} ({name})", app.appid),
        None => app.appid.clone(),
    };
    println!("  {} {}", "•".cyan().bold(), title);

    if let Ok(exe) = read_trimmed(app.path.join(EXE_FILE)) {
        if Path::new(&exe).exists() {
            doctor_ok(&format!("exe: {exe}"));
        } else {
            *errors += 1;
            doctor_err(&format!("exe path does not exist: {exe}"));
        }
    } else {
        *errors += 1;
        doctor_err("exe file is missing or unreadable");
    }

    if let Ok(pfx) = read_trimmed(app.path.join(PFX_FILE)) {
        if Path::new(&pfx).exists() {
            doctor_ok(&format!("pfx: {pfx}"));
        } else {
            *warnings += 1;
            doctor_warn(&format!("pfx path does not exist: {pfx}"));
        }
    } else {
        *warnings += 1;
        doctor_warn("pfx file is missing or unreadable");
    }

    if let Ok(env_content) = fs::read_to_string(app.path.join(ENV_FILE)) {
        doctor_ok("env: environment file is readable");
        match get_env_var(&env_content, STEAM_COMPAT_DATA_PATH_ENV) {
            Some(compat_data) if Path::new(&compat_data).exists() => {
                doctor_ok(&format!("env.STEAM_COMPAT_DATA_PATH: {compat_data}"));
            }
            Some(compat_data) => {
                *warnings += 1;
                doctor_warn(&format!(
                    "env.STEAM_COMPAT_DATA_PATH points to a missing path: {compat_data}"
                ));
            }
            None => {
                *warnings += 1;
                doctor_warn("env: STEAM_COMPAT_DATA_PATH is missing");
            }
        }
    } else {
        *errors += 1;
        doctor_err("env file is missing or unreadable");
    }

    if let Some(started_at) = app.started_at {
        doctor_ok(&format!(
            "started_at: {started_at} ({})",
            format_duration_ago(started_at)
        ));
    } else {
        *warnings += 1;
        doctor_warn("started_at is missing or invalid");
    }
}

fn doctor_ok(message: &str) {
    println!("    {} {message}", "OK".green().bold());
}

fn doctor_warn(message: &str) {
    println!("    {} {message}", "WARN".yellow().bold());
}

fn doctor_err(message: &str) {
    println!("    {} {message}", "ERR".red().bold());
}

fn doctor_info(message: &str) {
    println!("    {} {message}", "INFO".cyan().bold());
}

fn write_env_file(app_dir: &Path) -> io::Result<()> {
    let env_path = app_dir.join(ENV_FILE);
    let mut env_file = BufWriter::new(fs::File::create(env_path)?);
    let mut vars: Vec<_> = env::vars().collect();
    vars.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    for (key, value) in vars {
        let escaped_value = shell_escape(&value);
        writeln!(env_file, "declare -x {key}={escaped_value}")?;
    }

    Ok(())
}

fn read_trimmed<P: AsRef<Path>>(path: P) -> io::Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_string())
}

fn read_started_at(app_dir: &Path) -> Option<u64> {
    let val = fs::read_to_string(app_dir.join(STARTED_AT_FILE)).ok()?;
    val.trim().parse::<u64>().ok()
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

fn required_env_var(name: &str, command: &str) -> String {
    match env::var(name) {
        Ok(value) => value,
        Err(_) => print_subcommand_usage_error(command, &format!("{name} is not set")),
    }
}

fn print_subcommand_usage_error(subcommand: &str, message: &str) -> ! {
    eprintln!("{} {message}", "Error:".bold().red());
    sub_usage(subcommand);
    process::exit(1);
}

fn exit_with_status(status: process::ExitStatus) -> ! {
    process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::contains_case_insensitive;

    #[test]
    fn case_insensitive_search() {
        assert!(contains_case_insensitive("Gunfire Reborn", "gunfire"));
        assert!(contains_case_insensitive("GUNFIRE REBORN", "reborn"));
        assert!(!contains_case_insensitive("Gunfire Reborn", "helldivers"));
    }
}
