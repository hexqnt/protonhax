use std::{
    env, io,
    path::{Path, PathBuf},
    process,
};

use colored::Colorize;

use crate::{
    app_id::AppId,
    context::PendingContext,
    env_store::StoredEnv,
    proton::ProtonInvocation,
    runtime::unix_now_secs,
    shell::split_env_assignment,
    steam::{STEAM_APP_ID_ENV, STEAM_COMPAT_DATA_PATH_ENV},
};

use super::{exit_with_status, print_subcommand_usage_error};

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
    command.envs(
        init_command
            .env_assignments()
            .iter()
            .filter_map(|assignment| split_env_assignment(assignment)),
    );

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
        .position(|argument| split_env_assignment(argument).is_none())
    else {
        print_subcommand_usage_error("init", "No command specified after environment assignments");
    };

    InitCommand {
        tokens,
        cmd_start_index,
    }
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

#[cfg(test)]
mod tests {
    use super::parse_init_command;

    #[test]
    fn init_preserves_shell_quoting_and_environment_prefix_boundary() {
        let parsed = parse_init_command(vec![
            "PATH=/first PATH=/second LABEL='two words' /tools/proton run 'program name.exe' ARG=value"
                .to_owned(),
        ]);
        assert_eq!(
            parsed.env_assignments(),
            ["PATH=/first", "PATH=/second", "LABEL=two words"]
        );
        assert_eq!(
            parsed.command(),
            ["/tools/proton", "run", "program name.exe", "ARG=value"]
        );
    }
}
