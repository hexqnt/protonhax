#![warn(clippy::pedantic)]

use std::{env, io};

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;

use crate::cli::{Cli, Commands, ProfileCommands};
use crate::runtime::{debug_enabled, runtime_root};

mod app_id;
mod cli;
mod context;
mod doctor;
mod env_store;
mod flatpak;
mod handlers;
mod launch;
mod profile;
mod proton;
mod runtime;
mod shell;
mod steam;

fn main() -> io::Result<()> {
    let debug = debug_enabled();
    if debug {
        eprintln!(
            "{} Protonhax started with args: {:?}",
            "DEBUG".bold().cyan(),
            env::args_os().collect::<Vec<_>>()
        );
    }

    let Cli { command, .. } = Cli::parse();
    let Some(command) = command else {
        println!("protonhax {}\n", env!("CARGO_PKG_VERSION"));
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    let phd = runtime_root()?;

    match command {
        Commands::Init { cmd } => handlers::handle_init(&phd, cmd, debug),
        Commands::Ls { long, json } => handlers::handle_ls(&phd, long, json),
        Commands::Run {
            target,
            options,
            cmd,
        } => handlers::handle_run(&phd, &target, &cmd, &options.into()),
        Commands::Cmd { target } => handlers::handle_cmd(&phd, &target),
        Commands::Exec {
            target,
            environment,
            cmd,
        } => handlers::handle_exec(&phd, &target, &cmd, &environment.into()),
        Commands::Env { target } => handlers::handle_env(&phd, &target),
        Commands::Info { target, json } => handlers::handle_info(&phd, &target, json),
        Commands::Profile { command } => match command {
            ProfileCommands::Add {
                name,
                target,
                kind,
                options,
                cmd,
            } => profile::add(
                &name,
                profile::Profile::new(target, kind, cmd, options.into())?,
            ),
            ProfileCommands::Run { name, args } => handlers::handle_profile_run(&phd, &name, &args),
            ProfileCommands::Ls { json } => profile::list(json),
            ProfileCommands::Remove { name } => profile::remove(&name),
        },
        Commands::Doctor { json, fix } => doctor::handle_doctor(&phd, json, fix),
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "protonhax", &mut io::stdout());
            Ok(())
        }
    }
}
