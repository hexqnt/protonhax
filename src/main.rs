#![warn(clippy::pedantic)]

use clap::CommandFactory;
use clap::{Parser, Subcommand};
use clap_complete::{generate, shells::Shell as CompleteShell};
use colored::Colorize;
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

/// Функция для получения пути к директории protonhax.
fn get_phd() -> PathBuf {
    // Получаем XDG_RUNTIME_DIR или fallback на /run/user/<uid>.
    let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        // Получаем UID через команду id -u.
        let output = process::Command::new("id")
            .arg("-u")
            .output()
            .expect("Не удалось получить UID");
        let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        format!("/run/user/{uid}")
    });
    PathBuf::from(runtime_dir).join("protonhax")
}

/// Вывод справки для конкретной подкоманды.
fn sub_usage(sub: &str) {
    let mut cmd = Cli::command();
    if let Some(sc) = cmd.find_subcommand_mut(sub) {
        let _ = sc.print_help();
        println!();
    } else {
        // Fallback — общая справка
        let _ = cmd.print_help();
        println!();
    }
}

fn is_env_assignment(s: &str) -> bool {
    // Detect leading VAR=VALUE shell-style assignment (VAR must match [A-Za-z_][A-Za-z0-9_]*).
    if let Some(eq) = s.find('=') {
        let (name, _) = s.split_at(eq);
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
            _ => return false,
        }
        for c in chars {
            if !(c == '_' || c.is_ascii_alphanumeric()) {
                return false;
            }
        }
        return true;
    }
    false
}

/// Функция для экранирования строки в стиле shell для двойных кавычек.
fn shell_escape(s: &str) -> String {
    if s.contains(char::is_whitespace) || s.contains('\'') || s.contains('"') || s.contains('$') {
        let mut res = String::from("\"");
        for c in s.chars() {
            match c {
                '\\' | '"' | '$' | '`' => res.push('\\'),
                _ => {}
            }
            res.push(c);
        }
        res.push('"');
        res
    } else {
        s.to_string()
    }
}

/// Функция для деэкранирования строки в стиле shell из двойных кавычек.
fn un_shell_escape(s: &str) -> String {
    // Если строка не в кавычках, возвращаем как есть.
    if !s.starts_with('"') || !s.ends_with('"') {
        return s.to_string();
    }
    let s = &s[1..s.len() - 1];
    let mut res = String::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        chars.next(); // Потребляем символ.
        if c == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    '\\' => res.push('\\'),
                    '"' => res.push('"'),
                    '$' => res.push('$'),
                    '`' => res.push('`'),
                    _ => {
                        res.push('\\');
                        res.push(next);
                    }
                }
            } else {
                res.push('\\');
            }
        } else {
            res.push(c);
        }
    }
    res
}

#[derive(Parser)]
#[command(
    name = "protonhax",
    about = "Tool to help running other programs inside Steam's proton."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Should only be called by Steam with "protonhax init %COMMAND%"
    Init {
        /// The command to initialize with (e.g., the original %COMMAND%)
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Lists all currently running games
    Ls,
    /// Runs <cmd> in the context of <appid> with proton
    Run {
        /// The appid of the running game
        appid: String,
        /// The command to run with proton
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Runs cmd.exe in the context of <appid>
    Cmd {
        /// The appid of the running game
        appid: String,
    },
    /// Runs <cmd> in the context of <appid>
    Exec {
        /// The appid of the running game
        appid: String,
        /// The command to execute natively
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Generate shell completion scripts
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: CompleteShell,
    },
}

fn main() -> io::Result<()> {
    let debug = debug_enabled();
    if debug {
        eprintln!(
            "{} {}",
            "DEBUG".bold().cyan(),
            format!(
                "Protonhax started with args: {:?}",
                env::args().collect::<Vec<String>>()
            )
            .dimmed()
        );
    }
    let cli = Cli::parse();

    let phd = get_phd();

    match cli.command {
        Commands::Init { cmd } => {
            if cmd.is_empty() {
                sub_usage("init");
                process::exit(1);
            }
            // Проверяем наличие SteamAppId.
            let appid = env::var("SteamAppId").expect("SteamAppId не установлен");
            let app_dir = phd.join(&appid);
            fs::create_dir_all(&app_dir)?;

            // Steam иногда прокидывает %COMMAND% как одну строку. Разберём её, если это так.
            let cmd_tokens: Vec<String> =
                if cmd.len() == 1 && cmd[0].contains(|c: char| c.is_whitespace()) {
                    match shell_words::split(&cmd[0]) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!(
                                "{} {}",
                                "Ошибка:".bold().red(),
                                format!("Не удалось разобрать команду: {e}")
                            );
                            sub_usage("init");
                            process::exit(1);
                        }
                    }
                } else {
                    cmd.clone()
                };

            // Находим индекс начала настоящей команды (после возможных присваиваний VAR=VALUE)
            let cmd_start_index_opt = cmd_tokens.iter().position(|arg| !is_env_assignment(arg));
            if cmd_start_index_opt.is_none() {
                eprintln!(
                    "{} {}",
                    "Ошибка:".bold().red(),
                    "Не указана команда для запуска после присваиваний окружения"
                );
                sub_usage("init");
                process::exit(1);
            }
            let cmd_start_index = cmd_start_index_opt.unwrap();
            // Извлекаем настоящую команду (argv)
            let real_cmd = &cmd_tokens[cmd_start_index..];

            // Находим путь к proton в аргументах.
            let proton_path = if let Some(p) = real_cmd.iter().find(|a| a.contains("/proton")) {
                p.clone()
            } else {
                eprintln!(
                    "{} {}",
                    "Ошибка:".bold().red(),
                    "Путь к proton не найден в команде"
                );
                sub_usage("init");
                process::exit(1);
            };

            // Сохраняем данные
            fs::write(app_dir.join("exe"), &proton_path)?;

            // Сохраняем путь к pfx.
            let compat_data =
                env::var("STEAM_COMPAT_DATA_PATH").expect("STEAM_COMPAT_DATA_PATH не установлен");
            let pfx = format!("{compat_data}/pfx");
            fs::write(app_dir.join("pfx"), &pfx)?;

            // Сохраняем окружение в формате declare -x.
            let env_path = app_dir.join("env");
            let mut env_file = fs::File::create(&env_path)?;
            for (key, value) in env::vars() {
                let escaped_value = shell_escape(&value);
                writeln!(env_file, "declare -x {key}={escaped_value}")?;
            }

            // Выполняем исходную команду, учитывая возможные префиксные VAR=VALUE присваивания.
            // Первые аргументы вида KEY=VALUE должны стать переменными окружения дочернего процесса,
            // а не позиционными аргументами (как делает shell). Это поведение повторяет `exec "$@"` в protonhax.sh
            // без участия дополнительного парсинга через `sh -c`.
            let mut child = process::Command::new(&real_cmd[0]);
            child.args(&real_cmd[1..]);
            // Применяем присваивания окружения к дочернему процессу.
            for assign in &cmd_tokens[..cmd_start_index] {
                if let Some(eq_idx) = assign.find('=') {
                    let (k, v) = assign.split_at(eq_idx);
                    // split_at оставляет '=' в начале v, убираем его.
                    let v = &v[1..];
                    child.env(k, v);
                }
            }
            if debug {
                eprintln!(
                    "{} {}",
                    "DEBUG".bold().cyan(),
                    format!("Executing command (argv): {real_cmd:?}").dimmed()
                );
            }
            let status = child.status()?;
            let exit_code = status.code().unwrap_or(1);

            // Удаляем директорию.
            let _ = fs::remove_dir_all(&app_dir);

            process::exit(exit_code);
        }
        Commands::Ls => {
            if phd.exists() {
                for entry in fs::read_dir(&phd)? {
                    let entry = entry?;
                    if entry.path().is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        println!("{}", name.green());
                    }
                }
            }
            Ok(())
        }
        Commands::Run { appid, cmd } => {
            if cmd.is_empty() {
                sub_usage("run");
                process::exit(1);
            }
            let app_dir = phd.join(&appid);
            if !app_dir.exists() {
                eprintln!(
                    "{} {}",
                    "Ошибка:".bold().red(),
                    format!("Нет запущенного приложения с appid \"{appid}\"")
                );
                process::exit(2);
            }

            // Устанавливаем SteamAppId.
            unsafe {
                env::set_var("SteamAppId", &appid);
            }

            // Загружаем и применяем окружение из файла.
            load_env(&app_dir)?;

            // Выполняем команду.
            let exe = fs::read_to_string(app_dir.join("exe"))?.trim().to_string();
            let status = process::Command::new(exe).arg("run").args(&cmd).status()?;
            let exit_code = status.code().unwrap_or(1);
            process::exit(exit_code);
        }
        Commands::Cmd { appid } => {
            let app_dir = phd.join(&appid);
            if !app_dir.exists() {
                eprintln!(
                    "{} {}",
                    "Ошибка:".bold().red(),
                    format!("Нет запущенного приложения с appid \"{appid}\"")
                );
                process::exit(2);
            }

            // Устанавливаем SteamAppId.
            unsafe {
                env::set_var("SteamAppId", &appid);
            }

            // Загружаем и применяем окружение из файла.
            load_env(&app_dir)?;

            // Выполняем cmd.exe.
            let exe = fs::read_to_string(app_dir.join("exe"))?.trim().to_string();
            let pfx = fs::read_to_string(app_dir.join("pfx"))?.trim().to_string();
            let cmd_exe = format!("{pfx}/drive_c/windows/system32/cmd.exe");
            let status = process::Command::new(exe)
                .arg("run")
                .arg(cmd_exe)
                .status()?;
            let exit_code = status.code().unwrap_or(1);
            process::exit(exit_code);
        }
        Commands::Exec { appid, cmd } => {
            if cmd.is_empty() {
                sub_usage("exec");
                process::exit(1);
            }
            let app_dir = phd.join(&appid);
            if !app_dir.exists() {
                eprintln!(
                    "{} {}",
                    "Ошибка:".bold().red(),
                    format!("Нет запущенного приложения с appid \"{appid}\"")
                );
                process::exit(2);
            }

            // Устанавливаем SteamAppId.
            unsafe {
                env::set_var("SteamAppId", &appid);
            }

            // Загружаем и применяем окружение из файла.
            load_env(&app_dir)?;

            // Выполняем команду.
            let status = process::Command::new(&cmd[0]).args(&cmd[1..]).status()?;
            let exit_code = status.code().unwrap_or(1);
            process::exit(exit_code);
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "protonhax", &mut io::stdout());
            Ok(())
        }
    }
}

fn load_env<P: AsRef<Path>>(app_dir: P) -> Result<(), io::Error> {
    let app_dir = app_dir.as_ref();
    let env_content = fs::read_to_string(app_dir.join("env"))?;
    for line in env_content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("declare -x ")
            && let Some(eq_idx) = rest.find('=')
        {
            let name = rest[..eq_idx].trim().to_string();
            let value_str = rest[eq_idx + 1..].trim();
            let value = un_shell_escape(value_str);
            unsafe {
                env::set_var(name, value);
            }
        }
    }
    Ok(())
}
fn debug_enabled() -> bool {
    env::var_os("PROTONHAX_DEBUG").is_some()
}
