use colored::Colorize;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use crate::{
    cli::sub_usage,
    env_store::{load_env, set_env_var},
    runtime::{format_duration_ago, unix_now_secs},
    shell::{is_env_assignment, shell_escape},
    steam::resolve_app_meta,
};

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

pub fn handle_ls(phd: &Path, long: bool) -> io::Result<()> {
    if !phd.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(phd)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let appid = entry.file_name().to_string_lossy().to_string();
        if !long {
            println!("{}", appid.green());
            continue;
        }

        let meta = resolve_app_meta(&path, &appid);
        let started_ago = read_started_ago(&path);

        let mut parts: Vec<String> = Vec::with_capacity(4);
        parts.push(appid.green().to_string());

        if let Some(name) = meta.name {
            parts.push(name.yellow().to_string());
        }
        if let Some(install_path) = meta.install_path {
            parts.push(install_path.dimmed().to_string());
        }
        if let Some(ago) = started_ago {
            parts.push(format!("started {ago}").dimmed().to_string());
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

    let app_dir = prepare_context(phd, appid)?;
    let exe = read_trimmed(app_dir.join("exe"))?;
    let status = process::Command::new(exe).arg("run").args(cmd).status()?;

    process::exit(status.code().unwrap_or(1));
}

pub fn handle_cmd(phd: &Path, appid: &str) -> io::Result<()> {
    let app_dir = prepare_context(phd, appid)?;
    let exe = read_trimmed(app_dir.join("exe"))?;
    let pfx = read_trimmed(app_dir.join("pfx"))?;
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

    let _app_dir = prepare_context(phd, appid)?;
    let status = process::Command::new(&cmd[0]).args(&cmd[1..]).status()?;
    process::exit(status.code().unwrap_or(1));
}

fn prepare_context(phd: &Path, appid: &str) -> io::Result<PathBuf> {
    let app_dir = require_running_app(phd, appid);
    set_env_var("SteamAppId", appid);
    load_env(&app_dir)?;
    Ok(app_dir)
}

fn require_running_app(phd: &Path, appid: &str) -> PathBuf {
    let app_dir = phd.join(appid);
    if !app_dir.exists() {
        eprintln!(
            "{} Нет запущенного приложения с appid \"{appid}\"",
            "Ошибка:".bold().red()
        );
        process::exit(2);
    }
    app_dir
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

fn read_trimmed(path: PathBuf) -> io::Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_string())
}

fn read_started_ago(app_dir: &Path) -> Option<String> {
    let Ok(val) = fs::read_to_string(app_dir.join("started_at")) else {
        return None;
    };

    let Ok(secs) = val.trim().parse::<u64>() else {
        return None;
    };

    Some(format_duration_ago(secs))
}
