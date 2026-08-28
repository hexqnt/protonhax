use std::{
    fmt,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
};

use crate::shell::{EnvName, EnvOverride};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunCwd {
    ExeDir,
    Path(PathBuf),
}

impl RunCwd {
    pub fn apply_to(&self, command: &mut Command, executable: &str) {
        match self {
            Self::ExeDir => {
                let executable = Path::new(executable);
                let directory = executable
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty());
                let directory = directory.unwrap_or_else(|| Path::new("."));
                command.current_dir(directory);
            }
            Self::Path(path) => {
                command.current_dir(path);
            }
        }
    }
}

impl FromStr for RunCwd {
    type Err = ParseRunCwdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "" => Err(ParseRunCwdError),
            "exe-dir" => Ok(Self::ExeDir),
            path => Ok(Self::Path(PathBuf::from(path))),
        }
    }
}

impl fmt::Display for RunCwd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExeDir => formatter.write_str("exe-dir"),
            Self::Path(path) => path.display().fmt(formatter),
        }
    }
}

#[derive(Debug)]
pub struct ParseRunCwdError;

impl std::error::Error for ParseRunCwdError {}

impl fmt::Display for ParseRunCwdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("working directory must not be empty")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EnvironmentChanges {
    pub overrides: Vec<EnvOverride>,
    pub removals: Vec<EnvName>,
}

impl EnvironmentChanges {
    pub fn apply_to(&self, command: &mut Command) {
        for assignment in &self.overrides {
            command.env(assignment.name().as_str(), assignment.value());
        }
        for name in &self.removals {
            command.env_remove(name.as_str());
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunOptions {
    pub detach: bool,
    pub cwd: Option<RunCwd>,
    pub environment: EnvironmentChanges,
}

pub fn configure_detached(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::{EnvironmentChanges, RunCwd};

    #[test]
    fn executable_directory_uses_parent_path() {
        let mut command = Command::new("true");
        RunCwd::ExeDir.apply_to(&mut command, "/tmp/tools/trainer.exe");
        assert_eq!(
            command.get_current_dir(),
            Some(std::path::Path::new("/tmp/tools"))
        );
    }

    #[test]
    fn environment_changes_have_deterministic_order() {
        let changes = EnvironmentChanges {
            overrides: vec!["A=one".parse().unwrap(), "B=two".parse().unwrap()],
            removals: vec!["B".parse().unwrap()],
        };
        let mut command = Command::new("true");
        changes.apply_to(&mut command);

        let vars: Vec<_> = command.get_envs().collect();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[1], (std::ffi::OsStr::new("B"), None));
    }
}
