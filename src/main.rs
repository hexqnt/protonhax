#![warn(clippy::pedantic)]

mod cli;
mod env_store;
mod handlers;
mod runtime;
mod shell;
mod steam;

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;
use std::{env, io};

use crate::cli::{Cli, Commands};
use crate::runtime::{debug_enabled, runtime_root};

fn main() -> io::Result<()> {
    let debug = debug_enabled();
    if debug {
        eprintln!(
            "{} Protonhax started with args: {:?}",
            "DEBUG".bold().cyan(),
            env::args().collect::<Vec<String>>()
        );
    }

    let cli = Cli::parse();
    let phd = runtime_root();

    match cli.command {
        Commands::Init { cmd } => handlers::handle_init(&phd, cmd, debug),
        Commands::Ls { long, json } => handlers::handle_ls(&phd, long, json),
        Commands::Run { appid, cmd } => handlers::handle_run(&phd, &appid, &cmd),
        Commands::Cmd { appid } => handlers::handle_cmd(&phd, &appid),
        Commands::Exec { appid, cmd } => handlers::handle_exec(&phd, &appid, &cmd),
        Commands::Doctor => handlers::handle_doctor(&phd),
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "protonhax", &mut io::stdout());
            Ok(())
        }
    }
}
