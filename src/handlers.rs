use colored::Colorize;
use serde_json::json;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use crate::{
    cli::sub_usage,
    env_store::{get_env_var, load_env, set_env_var},
    runtime::{format_duration_ago, unix_now_secs},
    shell::{is_env_assignment, shell_escape},
    steam::{AppMeta, resolve_app_meta},
};

struct RunningApp {
    appid: String,
    path: PathBuf,
    name: Option<String>,
    install_path: Option<String>,
    started_at: Option<u64>,
}

struct TargetApp {
    appid: String,
    app_dir: PathBuf,
}

pub fn handle_init(phd: &Path, cmd: Vec<String>, debug: bool) -> io::Result<()> {
    if cmd.is_empty() {
        sub_usage("init");
        process::exit(1);
    }

    let appid = env::var("SteamAppId").expect("SteamAppId не установлен");
    let app_dir = phd.join(&appid);
    fs::create_dir_all(&app_dir)?;

    // Сохраняем время старта (unix epoch, секунды).
    fs::write(app_dir.join("started_at"), unix_now_secs().to_string())?;

    // Steam иногда прокидывает %COMMAND% как одну строку. Разберём её, если это так.
    let cmd_tokens = if cmd.len() == 1 && cmd[0].contains(char::is_whitespace) {
        match shell_words::split(&cmd[0]) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "{} Не удалось разобрать команду: {e}",
                    "Ошибка:".bold().red()
                );
                sub_usage("init");
                process::exit(1);
            }
        }
    } else {
        cmd
    };

    // Находим индекс начала настоящей команды (после возможных присваиваний VAR=VALUE).
    let Some(cmd_start_index) = cmd_tokens.iter().position(|arg| !is_env_assignment(arg)) else {
        eprintln!(
            "{} Не указана команда для запуска после присваиваний окружения",
            "Ошибка:".bold().red()
        );
        sub_usage("init");
        process::exit(1);
    };

    let real_cmd = &cmd_tokens[cmd_start_index..];

    // Находим путь к proton в аргументах.
    let Some(proton_path) = real_cmd.iter().find(|arg| arg.contains("/proton")) else {
        eprintln!(
            "{} Путь к proton не найден в команде",
            "Ошибка:".bold().red()
        );
        sub_usage("init");
        process::exit(1);
    };

    // Сохраняем данные.
    fs::write(app_dir.join("exe"), proton_path)?;

    // Сохраняем путь к pfx.
    let compat_data =
        env::var("STEAM_COMPAT_DATA_PATH").expect("STEAM_COMPAT_DATA_PATH не установлен");
    fs::write(app_dir.join("pfx"), format!("{compat_data}/pfx"))?;

    // Сохраняем окружение в формате declare -x.
    write_env_file(&app_dir)?;

    // Выполняем исходную команду, учитывая возможные префиксные VAR=VALUE присваивания.
    let mut child = process::Command::new(&real_cmd[0]);
    child.args(&real_cmd[1..]);

    for assign in &cmd_tokens[..cmd_start_index] {
        if let Some((name, value)) = assign.split_once('=') {
            child.env(name, value);
        }
    }

    if debug {
        eprintln!(
            "{} Executing command (argv): {real_cmd:?}",
            "DEBUG".bold().cyan()
        );
    }

    let status = child.status()?;
    let exit_code = status.code().unwrap_or(1);

    // Удаляем директорию.
    let _ = fs::remove_dir_all(&app_dir);
    process::exit(exit_code);
}

pub fn handle_ls(phd: &Path, long: bool, json_output: bool) -> io::Result<()> {
    let apps = collect_running_apps(phd, long || json_output)?;

    if json_output {
        return print_ls_json(&apps);
    }

    for app in apps {
        if !long {
            println!("{}", app.appid.green());
            continue;
        }

        let mut parts: Vec<String> = Vec::with_capacity(4);
        parts.push(app.appid.green().to_string());

        if let Some(name) = app.name {
            parts.push(name.yellow().to_string());
        }
        if let Some(install_path) = app.install_path {
            parts.push(install_path.dimmed().to_string());
        }
        if let Some(started_at) = app.started_at {
            parts.push(
                format!("started {}", format_duration_ago(started_at))
                    .dimmed()
                    .to_string(),
            );
        }

        println!("{}", parts.join("  "));
    }

    Ok(())
}

pub fn handle_run(phd: &Path, appid: &str, cmd: &[String]) -> io::Result<()> {
    if cmd.is_empty() {
        sub_usage("run");
        process::exit(1);
    }

    let target = prepare_context(phd, appid)?;
    let exe = read_trimmed(target.app_dir.join("exe"))?;
    let status = process::Command::new(exe).arg("run").args(cmd).status()?;

    process::exit(status.code().unwrap_or(1));
}

pub fn handle_cmd(phd: &Path, appid: &str) -> io::Result<()> {
    let target = prepare_context(phd, appid)?;
    let exe = read_trimmed(target.app_dir.join("exe"))?;
    let pfx = read_trimmed(target.app_dir.join("pfx"))?;
    let cmd_exe = format!("{pfx}/drive_c/windows/system32/cmd.exe");

    let status = process::Command::new(exe)
        .arg("run")
        .arg(cmd_exe)
        .status()?;

    process::exit(status.code().unwrap_or(1));
}

pub fn handle_exec(phd: &Path, appid: &str, cmd: &[String]) -> io::Result<()> {
    if cmd.is_empty() {
        sub_usage("exec");
        process::exit(1);
    }

    let _target = prepare_context(phd, appid)?;
    let status = process::Command::new(&cmd[0]).args(&cmd[1..]).status()?;
    process::exit(status.code().unwrap_or(1));
}

pub fn handle_doctor(phd: &Path) -> io::Result<()> {
    let mut warnings = 0usize;
    let mut errors = 0usize;

    println!("{}", "protonhax doctor".bold());

    println!("\nEnvironment:");
    if let Ok(steam_app_id) = env::var("SteamAppId") {
        doctor_ok(&format!("SteamAppId={steam_app_id}"));
    } else {
        doctor_info("SteamAppId не установлен (это нормально вне запуска через Steam)");
    }

    match env::var("STEAM_COMPAT_DATA_PATH") {
        Ok(path) => {
            if Path::new(&path).exists() {
                doctor_ok(&format!("STEAM_COMPAT_DATA_PATH={path}"));
            } else {
                warnings += 1;
                doctor_warn(&format!(
                    "STEAM_COMPAT_DATA_PATH установлен, но путь не найден: {path}"
                ));
            }
        }
        Err(_) => {
            doctor_info("STEAM_COMPAT_DATA_PATH не установлен (это нормально вне запуска игры)");
        }
    }

    println!("\nRuntime:");
    if phd.exists() {
        doctor_ok(&format!("runtime root: {}", phd.display()));
    } else {
        warnings += 1;
        doctor_warn(&format!(
            "runtime root отсутствует: {} (ещё не было активных контекстов)",
            phd.display()
        ));
    }

    println!("\nContexts:");
    let apps = collect_running_apps(phd, true)?;
    if apps.is_empty() {
        warnings += 1;
        doctor_warn("активных контекстов не найдено");
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

fn prepare_context(phd: &Path, selector: &str) -> io::Result<TargetApp> {
    let target = resolve_target_app(phd, selector)?;
    set_env_var("SteamAppId", &target.appid);
    load_env(&target.app_dir)?;
    Ok(target)
}

fn resolve_target_app(phd: &Path, selector: &str) -> io::Result<TargetApp> {
    if selector.eq_ignore_ascii_case("latest") {
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
    let apps = collect_running_apps(phd, false)?;
    if apps.is_empty() {
        eprintln!(
            "{} Нет активных контекстов. Сначала запустите игру через Steam.",
            "Ошибка:".bold().red()
        );
        process::exit(2);
    }

    if let Some(app) = apps
        .iter()
        .filter_map(|app| app.started_at.map(|started_at| (started_at, app)))
        .max_by_key(|(started_at, _)| *started_at)
        .map(|(_, app)| app)
    {
        return Ok(TargetApp {
            appid: app.appid.clone(),
            app_dir: app.path.clone(),
        });
    }

    if apps.len() == 1 {
        let app = &apps[0];
        return Ok(TargetApp {
            appid: app.appid.clone(),
            app_dir: app.path.clone(),
        });
    }

    eprintln!(
        "{} Невозможно определить latest: нет started_at у активных контекстов.",
        "Ошибка:".bold().red()
    );
    eprintln!("Укажите appid явно (см. `protonhax ls -l`).");
    process::exit(2);
}

fn resolve_app_by_name(phd: &Path, query: &str) -> io::Result<TargetApp> {
    let apps = collect_running_apps(phd, true)?;
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
                "{} Нет запущенного приложения с appid \"{query}\" и нет совпадений по имени.",
                "Ошибка:".bold().red()
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
        "{} Несколько совпадений по имени \"{query}\":",
        "Ошибка:".bold().red()
    );
    for app in matches {
        let name = app.name.as_deref().unwrap_or("<без названия>");
        eprintln!("  {}  {}", app.appid.green(), name.yellow());
    }
    eprintln!("Уточните appid через `protonhax ls -l`.");
}

fn collect_running_apps(phd: &Path, with_meta: bool) -> io::Result<Vec<RunningApp>> {
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

        let appid = entry.file_name().to_string_lossy().to_string();
        let meta = if with_meta {
            resolve_app_meta(&path, &appid)
        } else {
            AppMeta::default()
        };

        apps.push(RunningApp {
            appid,
            path: path.clone(),
            name: meta.name,
            install_path: meta.install_path,
            started_at: read_started_at(&path),
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
                "install_path": app.install_path,
                "started_at": app.started_at,
                "started_ago": app.started_at.map(format_duration_ago),
            })
        })
        .collect();

    let serialized = serde_json::to_string_pretty(&data).map_err(io::Error::other)?;
    println!("{serialized}");
    Ok(())
}

fn inspect_context(app: &RunningApp, warnings: &mut usize, errors: &mut usize) {
    let title = match app.name.as_deref() {
        Some(name) => format!("{} ({name})", app.appid),
        None => app.appid.clone(),
    };
    println!("  {} {}", "•".cyan().bold(), title);

    if let Ok(exe) = read_trimmed(app.path.join("exe")) {
        if Path::new(&exe).exists() {
            doctor_ok(&format!("exe: {exe}"));
        } else {
            *errors += 1;
            doctor_err(&format!("exe путь не существует: {exe}"));
        }
    } else {
        *errors += 1;
        doctor_err("файл exe отсутствует или не читается");
    }

    if let Ok(pfx) = read_trimmed(app.path.join("pfx")) {
        if Path::new(&pfx).exists() {
            doctor_ok(&format!("pfx: {pfx}"));
        } else {
            *warnings += 1;
            doctor_warn(&format!("pfx путь не существует: {pfx}"));
        }
    } else {
        *warnings += 1;
        doctor_warn("файл pfx отсутствует или не читается");
    }

    if let Ok(env_content) = fs::read_to_string(app.path.join("env")) {
        doctor_ok("env: файл окружения прочитан");
        match get_env_var(&env_content, "STEAM_COMPAT_DATA_PATH") {
            Some(compat_data) if Path::new(&compat_data).exists() => {
                doctor_ok(&format!("env.STEAM_COMPAT_DATA_PATH: {compat_data}"));
            }
            Some(compat_data) => {
                *warnings += 1;
                doctor_warn(&format!(
                    "env.STEAM_COMPAT_DATA_PATH указывает на отсутствующий путь: {compat_data}"
                ));
            }
            None => {
                *warnings += 1;
                doctor_warn("env: отсутствует STEAM_COMPAT_DATA_PATH");
            }
        }
    } else {
        *errors += 1;
        doctor_err("файл env отсутствует или не читается");
    }

    if let Some(started_at) = app.started_at {
        doctor_ok(&format!(
            "started_at: {started_at} ({})",
            format_duration_ago(started_at)
        ));
    } else {
        *warnings += 1;
        doctor_warn("started_at отсутствует или повреждён");
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
    let env_path = app_dir.join("env");
    let mut env_file = fs::File::create(env_path)?;

    for (key, value) in env::vars() {
        let escaped_value = shell_escape(&value);
        writeln!(env_file, "declare -x {key}={escaped_value}")?;
    }

    Ok(())
}

fn read_trimmed<P: AsRef<Path>>(path: P) -> io::Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_string())
}

fn read_started_at(app_dir: &Path) -> Option<u64> {
    let val = fs::read_to_string(app_dir.join("started_at")).ok()?;
    val.trim().parse::<u64>().ok()
}

fn contains_case_insensitive(text: &str, query: &str) -> bool {
    text.to_lowercase().contains(&query.to_lowercase())
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
