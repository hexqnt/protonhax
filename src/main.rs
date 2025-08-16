use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process;

use clap::CommandFactory;
use clap::{Parser, Subcommand};
use clap_complete::{generate, shells::Shell as CompleteShell};
// Функция для получения пути к директории protonhax.
fn get_phd() -> PathBuf {
    // Получаем XDG_RUNTIME_DIR или fallback на /run/user/<uid>.
    let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        // Получаем UID через команду id -u.
        let output = process::Command::new("id")
            .arg("-u")
            .output()
            .expect("Не удалось получить UID");
        let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        format!("/run/user/{}", uid)
    });
    PathBuf::from(runtime_dir).join("protonhax")
}

// Функция для вывода справки по использованию.
fn usage() {
    println!("Usage:");
    println!("protonhax init <cmd>");
    println!("\tShould only be called by Steam with \"protonhax init %COMMAND%\"");
    println!("protonhax ls");
    println!("\tLists all currently running games");
    println!("protonhax run <appid> <cmd>");
    println!("\tRuns <cmd> in the context of <appid> with proton");
    println!("protonhax cmd <appid>");
    println!("\tRuns cmd.exe in the context of <appid>");
    println!("protonhax exec <appid> <cmd>");
    println!("\tRuns <cmd> in the context of <appid>");
}

// Функция для экранирования строки в стиле shell для двойных кавычек.
fn shell_escape(s: &str) -> String {
    // Экранируем специальные символы: \, ", $, `.
    let mut res = String::with_capacity(s.len() + 2);
    res.push('"');
    for c in s.chars() {
        match c {
            '\\' => res.push_str("\\\\"),
            '"' => res.push_str("\\\""),
            '$' => res.push_str("\\$"),
            '`' => res.push_str("\\`"),
            _ => res.push(c),
        }
    }
    res.push('"');
    res
}

// Функция для деэкранирования строки в стиле shell из двойных кавычек.
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
        #[arg(required = true, num_args = 1..)]
        cmd: Vec<String>,
    },
    /// Lists all currently running games
    Ls,
    /// Runs <cmd> in the context of <appid> with proton
    Run {
        /// The appid of the running game
        appid: String,
        /// The command to run with proton
        #[arg(required = true, num_args = 1..)]
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
        #[arg(required = true, num_args = 1..)]
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
    let cli = Cli::parse();

    let phd = get_phd();

    match cli.command {
        Commands::Init { cmd } => {
            if cmd.is_empty() {
                usage();
                process::exit(1);
            }
            // Проверяем наличие SteamAppId.
            let appid = env::var("SteamAppId").expect("SteamAppId не установлен");
            let app_dir = phd.join(&appid);
            fs::create_dir_all(&app_dir)?;

            // Находим путь к proton в аргументах.
            let proton_path = cmd
                .iter()
                .find(|a| a.contains("/proton"))
                .expect("Путь к proton не найден")
                .clone();
            fs::write(app_dir.join("exe"), &proton_path)?;

            // Сохраняем путь к pfx.
            let compat_data =
                env::var("STEAM_COMPAT_DATA_PATH").expect("STEAM_COMPAT_DATA_PATH не установлен");
            let pfx = format!("{}/pfx", compat_data);
            fs::write(app_dir.join("pfx"), &pfx)?;

            // Сохраняем окружение в формате declare -x.
            let env_path = app_dir.join("env");
            let mut env_file = fs::File::create(&env_path)?;
            for (key, value) in env::vars() {
                let escaped_value = shell_escape(&value);
                writeln!(env_file, "declare -x {}={}", key, escaped_value)?;
            }

            // Запускаем оригинальную команду.
            let status = process::Command::new(&cmd[0]).args(&cmd[1..]).status()?;

            let exit_code = status.code().unwrap_or(1);

            // Удаляем директорию.
            fs::remove_dir_all(app_dir)?;

            process::exit(exit_code);
        }
        Commands::Ls => {
            if phd.exists() {
                for entry in fs::read_dir(&phd)? {
                    let entry = entry?;
                    if entry.path().is_dir() {
                        println!("{}", entry.file_name().to_string_lossy());
                    }
                }
            }
            Ok(())
        }
        Commands::Run { appid, cmd } => {
            if cmd.is_empty() {
                usage();
                process::exit(1);
            }
            let app_dir = phd.join(&appid);
            if !app_dir.exists() {
                eprintln!("Нет запущенного приложения с appid \"{}\"", appid);
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
                eprintln!("Нет запущенного приложения с appid \"{}\"", appid);
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
            let cmd_exe = format!("{}/drive_c/windows/system32/cmd.exe", pfx);
            let status = process::Command::new(exe)
                .arg("run")
                .arg(cmd_exe)
                .status()?;
            let exit_code = status.code().unwrap_or(1);
            process::exit(exit_code);
        }
        Commands::Exec { appid, cmd } => {
            if cmd.is_empty() {
                usage();
                process::exit(1);
            }
            let app_dir = phd.join(&appid);
            if !app_dir.exists() {
                eprintln!("Нет запущенного приложения с appid \"{}\"", appid);
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
    let _: () = for line in env_content.lines() {
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
    };
    Ok(())
}
