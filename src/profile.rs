use std::{
    ffi::OsStr,
    fmt,
    io::{self, Write},
    str::FromStr,
};

use clap::ValueEnum;
use serde_json::json;

use crate::{app_id::TargetSelector, env_store::is_sensitive_name, launch::RunOptions};

use self::store::Store;

mod store;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ProfileKind {
    Proton,
    Native,
}

impl FromStr for ProfileKind {
    type Err = io::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "proton" => Ok(Self::Proton),
            "native" => Ok(Self::Native),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid profile kind",
            )),
        }
    }
}

impl fmt::Display for ProfileKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Proton => "proton",
            Self::Native => "native",
        })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProfileName(String);

impl ProfileName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProfileName {
    type Err = ParseProfileNameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        (!value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
        .then(|| Self(value.to_owned()))
        .ok_or(ParseProfileNameError)
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug)]
pub struct ParseProfileNameError;

impl std::error::Error for ParseProfileNameError {}

impl fmt::Display for ParseProfileNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("profile name may contain only ASCII letters, digits, '_', '-' and '.'")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    pub target: TargetSelector,
    pub kind: ProfileKind,
    pub command: Vec<String>,
    pub run_options: RunOptions,
}

impl Profile {
    pub fn new(
        target: TargetSelector,
        kind: ProfileKind,
        command: Vec<String>,
        run_options: RunOptions,
    ) -> io::Result<Self> {
        if command.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "profile command must not be empty",
            ));
        }
        if kind == ProfileKind::Native && (run_options.detach || run_options.cwd.is_some()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--detach and --cwd are only supported by proton profiles",
            ));
        }
        Ok(Self {
            target,
            kind,
            command,
            run_options,
        })
    }
}

pub fn add(name: &ProfileName, profile: Profile) -> io::Result<()> {
    let mut store = Store::load()?;
    store.profiles.insert(name.clone(), profile);
    store.save()?;
    println!("Profile `{name}` saved.");
    Ok(())
}

pub fn remove(name: &ProfileName) -> io::Result<()> {
    let mut store = Store::load()?;
    if store.profiles.remove(name).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("profile `{name}` does not exist"),
        ));
    }
    store.save()?;
    println!("Profile `{name}` removed.");
    Ok(())
}

pub fn list(json_output: bool) -> io::Result<()> {
    let store = Store::load()?;
    if json_output {
        let profiles: Vec<_> = store
            .profiles
            .iter()
            .map(|(name, profile)| profile_json(name, profile))
            .collect();
        let stdout = io::stdout();
        let mut output = stdout.lock();
        serde_json::to_writer_pretty(&mut output, &profiles).map_err(io::Error::other)?;
        return writeln!(output);
    }

    for (name, profile) in store.profiles {
        println!(
            "{name}  {}  {}  {}",
            profile.kind,
            profile.target,
            profile.command.join(" ")
        );
    }
    Ok(())
}

pub fn resolve(name: &ProfileName, extra_args: &[String]) -> io::Result<Profile> {
    let mut profile = Store::load()?.profiles.remove(name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("profile `{name}` does not exist"),
        )
    })?;
    profile.command.extend_from_slice(extra_args);
    Ok(profile)
}

fn profile_json(name: &ProfileName, profile: &Profile) -> serde_json::Value {
    let environment: Vec<_> = profile
        .run_options
        .environment
        .overrides
        .iter()
        .map(|assignment| {
            if is_sensitive_name(OsStr::new(assignment.name().as_str())) {
                format!("{}=<redacted>", assignment.name())
            } else {
                assignment.to_string()
            }
        })
        .collect();
    let unset_environment: Vec<_> = profile
        .run_options
        .environment
        .removals
        .iter()
        .map(ToString::to_string)
        .collect();
    json!({
        "name": name.as_str(),
        "target": profile.target.to_string(),
        "kind": profile.kind.to_string(),
        "command": profile.command,
        "detach": profile.run_options.detach,
        "cwd": profile.run_options.cwd.as_ref().map(ToString::to_string),
        "environment": environment,
        "unset_environment": unset_environment,
    })
}

#[cfg(test)]
mod tests {
    use super::ProfileName;

    #[test]
    fn profile_name_is_safe_as_a_map_key() {
        assert!("trainer.main".parse::<ProfileName>().is_ok());
        assert!("../trainer".parse::<ProfileName>().is_err());
    }
}
