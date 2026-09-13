use clap::CommandFactory;
use clap::{Args, Parser, Subcommand};
use clap_complete::shells::Shell as CompleteShell;

use crate::app_id::TargetSelector;
use crate::{
    launch::{EnvironmentChanges, RunCwd, RunOptions},
    profile::{ProfileKind, ProfileName},
    shell::{EnvName, EnvOverride},
};

#[derive(Subcommand)]
pub enum Commands {
    /// Should only be called by Steam with "protonhax init %COMMAND%"
    #[command(after_help = "Examples:\n  protonhax init %COMMAND%")]
    Init {
        /// The command to initialize with (e.g., the original %COMMAND%)
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Lists all currently running games
    Ls {
        /// Show extra details (prefix, install path, Proton, start time)
        #[arg(short = 'l', long = "long")]
        long: bool,
        /// Output as JSON
        #[arg(long = "json")]
        json: bool,
    },
    /// Runs <cmd> as a Windows application in the context of <target> with Proton
    #[command(after_help = "\
Examples:
  protonhax run latest ~/Downloads/trainer.exe
  protonhax run 1217060 --detach --cwd exe-dir ~/Downloads/trainer.exe
  protonhax run gunfire --env WINEDEBUG=-all -- trainer.exe --silent")]
    Run {
        /// Target game: appid, `latest`, or part of game name
        target: TargetSelector,
        #[command(flatten)]
        options: RunOptionsArgs,
        /// The command to run Windows applications with proton
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Runs cmd.exe in the context of <target>
    Cmd {
        /// Target game: appid, `latest`, or part of game name
        target: TargetSelector,
    },
    /// Runs <cmd> as a Linux application in the context of <target>
    #[command(after_help = "\
Examples:
  protonhax exec latest env
  protonhax exec gunfire --env MANGOHUD=1 --unset-env WINEDEBUG -- mangohud --help")]
    Exec {
        /// Target game: appid, `latest`, or part of game name
        target: TargetSelector,
        #[command(flatten)]
        environment: EnvironmentArgs,
        /// The command to execute Linux applications natively without proton prefix
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Prints the stored environment for <target>
    Env {
        /// Target game: appid, `latest`, or part of game name
        target: TargetSelector,
    },
    /// Shows detailed information about one active context
    Info {
        /// Target game: appid, `latest`, or part of game name
        target: TargetSelector,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage saved command profiles
    #[command(after_help = "\
Examples:
  protonhax profile add trainer latest -- ~/Downloads/trainer.exe
  protonhax profile run trainer
  protonhax profile ls")]
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
    /// Generate shell completion scripts
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: CompleteShell,
    },
    /// Validate current runtime contexts and environment
    Doctor {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Remove stale contexts and correct runtime permissions
        #[arg(long)]
        fix: bool,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommands {
    /// Save or replace a profile
    #[command(after_help = "\
Examples:
  protonhax profile add trainer latest --detach --cwd exe-dir -- ~/Downloads/trainer.exe
  protonhax profile add game-env latest --kind native -- env")]
    Add {
        /// Profile name
        name: ProfileName,
        /// Target game: appid, `latest`, or part of game name
        target: TargetSelector,
        /// Command kind
        #[arg(long, value_enum, default_value_t = ProfileKind::Proton)]
        kind: ProfileKind,
        #[command(flatten)]
        options: RunOptionsArgs,
        /// Command saved in the profile
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Run a saved profile and optionally append arguments
    #[command(after_help = "\
Examples:
  protonhax profile run trainer
  protonhax profile run trainer -- --silent")]
    Run {
        /// Profile name
        name: ProfileName,
        /// Arguments appended to the saved command
        #[arg(num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// List saved profiles
    Ls {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a saved profile
    Remove {
        /// Profile name
        name: ProfileName,
    },
}

#[derive(Args)]
pub struct EnvironmentArgs {
    /// Override an environment variable (repeatable)
    #[arg(long = "env", value_name = "NAME=VALUE")]
    overrides: Vec<EnvOverride>,
    /// Remove an environment variable (repeatable)
    #[arg(long = "unset-env", value_name = "NAME")]
    removals: Vec<EnvName>,
}

#[derive(Args)]
pub struct RunOptionsArgs {
    /// Run without waiting and disconnect standard streams
    #[arg(long)]
    detach: bool,
    /// Working directory: a path or `exe-dir`
    #[arg(long, value_name = "PATH|exe-dir")]
    cwd: Option<RunCwd>,
    #[command(flatten)]
    environment: EnvironmentArgs,
}

#[derive(Parser)]
#[command(
    name = "protonhax",
    version,
    disable_version_flag = true,
    about = "Run other programs inside a Steam Proton environment.",
    after_help = "\
Examples:
  protonhax ls --long
  protonhax run latest ~/Downloads/trainer.exe
  protonhax cmd latest
  protonhax doctor

Run `protonhax <command> --help` for command-specific examples."
)]
pub struct Cli {
    /// Print version
    #[arg(
        short = 'v',
        long = "version",
        action = clap::ArgAction::Version
    )]
    version: Option<bool>,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl From<EnvironmentArgs> for EnvironmentChanges {
    fn from(args: EnvironmentArgs) -> Self {
        Self {
            overrides: args.overrides,
            removals: args.removals,
        }
    }
}

impl From<RunOptionsArgs> for RunOptions {
    fn from(args: RunOptionsArgs) -> Self {
        Self {
            detach: args.detach,
            cwd: args.cwd,
            environment: args.environment.into(),
        }
    }
}

/// Вывод справки для конкретной подкоманды.
pub fn sub_usage(sub: &str) {
    let mut cmd = Cli::command();
    if let Some(sc) = cmd.find_subcommand_mut(sub) {
        let _ = sc.print_help();
        println!();
    } else {
        // Резервный вывод общей справки.
        let _ = cmd.print_help();
        println!();
    }
}
