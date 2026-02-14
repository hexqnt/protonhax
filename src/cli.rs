use clap::CommandFactory;
use clap::{Parser, Subcommand};
use clap_complete::shells::Shell as CompleteShell;

#[derive(Parser)]
#[command(
    name = "protonhax",
    about = "Tool to help running other programs inside Steam's proton."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Should only be called by Steam with "protonhax init %COMMAND%"
    Init {
        /// The command to initialize with (e.g., the original %COMMAND%)
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Lists all currently running games
    Ls {
        /// Show extra details (name, install path)
        #[arg(short = 'l', long = "long")]
        long: bool,
    },
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

/// Вывод справки для конкретной подкоманды.
pub fn sub_usage(sub: &str) {
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
