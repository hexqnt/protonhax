use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    launch::{EnvironmentChanges, RunOptions},
    runtime::current_uid,
};

use super::{Profile, ProfileName};

const CONFIG_FILE: &str = "profiles.json";
const CONFIG_VERSION: u32 = 1;
const MAX_CONFIG_SIZE: u64 = 1024 * 1024;

#[derive(Default)]
pub(super) struct Store {
    pub(super) profiles: BTreeMap<ProfileName, Profile>,
}

impl Store {
    pub(super) fn load() -> io::Result<Self> {
        let path = config_root()?.join(CONFIG_FILE);
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_SIZE + 1).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFIG_SIZE {
            return Err(invalid_data("profile configuration is too large"));
        }
        Self::decode(&bytes)
    }

    pub(super) fn decode(bytes: &[u8]) -> io::Result<Self> {
        let dto: StoreDto = serde_json::from_slice(bytes).map_err(invalid_json)?;
        if dto.version != CONFIG_VERSION {
            return Err(invalid_data("unsupported profile configuration version"));
        }

        let profiles = dto
            .profiles
            .into_iter()
            .map(|(name, profile)| Ok((name.parse().map_err(invalid_parse)?, profile.try_into()?)))
            .collect::<io::Result<_>>()?;
        Ok(Self { profiles })
    }

    pub(super) fn save(&self) -> io::Result<()> {
        let root = ensure_config_root()?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = root.join(format!(".{CONFIG_FILE}.{}-{nonce}", process::id()));
        let destination = root.join(CONFIG_FILE);
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            serde_json::to_writer_pretty(&mut file, &StoreDto::from(self))
                .map_err(io::Error::other)?;
            writeln!(file)?;
            file.sync_all()?;
            fs::rename(&temporary, destination)?;
            fs::File::open(root)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

#[derive(Deserialize, Serialize)]
struct StoreDto {
    version: u32,
    profiles: BTreeMap<String, ProfileDto>,
}

impl From<&Store> for StoreDto {
    fn from(store: &Store) -> Self {
        Self {
            version: CONFIG_VERSION,
            profiles: store
                .profiles
                .iter()
                .map(|(name, profile)| (name.to_string(), ProfileDto::from(profile)))
                .collect(),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct ProfileDto {
    target: String,
    kind: String,
    command: Vec<String>,
    detach: bool,
    cwd: Option<String>,
    environment: Vec<String>,
    unset_environment: Vec<String>,
}

impl From<&Profile> for ProfileDto {
    fn from(profile: &Profile) -> Self {
        Self {
            target: profile.target.to_string(),
            kind: profile.kind.to_string(),
            command: profile.command.clone(),
            detach: profile.run_options.detach,
            cwd: profile.run_options.cwd.as_ref().map(ToString::to_string),
            environment: profile
                .run_options
                .environment
                .overrides
                .iter()
                .map(ToString::to_string)
                .collect(),
            unset_environment: profile
                .run_options
                .environment
                .removals
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
    }
}

impl TryFrom<ProfileDto> for Profile {
    type Error = io::Error;

    fn try_from(dto: ProfileDto) -> Result<Self, Self::Error> {
        Self::new(
            dto.target.parse().map_err(invalid_parse)?,
            dto.kind.parse()?,
            dto.command,
            RunOptions {
                detach: dto.detach,
                cwd: dto
                    .cwd
                    .map(|cwd| cwd.parse().map_err(invalid_parse))
                    .transpose()?,
                environment: EnvironmentChanges {
                    overrides: parse_all(dto.environment)?,
                    removals: parse_all(dto.unset_environment)?,
                },
            },
        )
    }
}

fn config_root() -> io::Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env::var_os("HOME").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "neither XDG_CONFIG_HOME nor HOME is set",
            )
        })?)
        .join(".config"),
    };
    if !base.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration directory must be an absolute path",
        ));
    }
    Ok(base.join("protonhax"))
}

fn ensure_config_root() -> io::Result<PathBuf> {
    let root = config_root()?;
    if let Some(parent) = root.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::symlink_metadata(&root) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(invalid_data(
                    "profile configuration path is not a directory",
                ));
            }
            if metadata.uid() != current_uid()? {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "profile configuration belongs to another user",
                ));
            }
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(0o700).create(&root)?;
        }
        Err(error) => return Err(error),
    }
    Ok(root)
}

fn parse_all<T>(values: Vec<String>) -> io::Result<Vec<T>>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    values
        .into_iter()
        .map(|value| value.parse().map_err(invalid_parse))
        .collect()
}

fn invalid_json(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_parse(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::profile::{ProfileKind, ProfileName};

    #[test]
    fn configuration_is_parsed_into_typed_values() {
        let data = br#"{
            "version": 1,
            "profiles": {
                "trainer": {
                    "target": "latest",
                    "kind": "proton",
                    "command": ["/tmp/trainer.exe"],
                    "detach": true,
                    "cwd": "exe-dir",
                    "environment": ["DXVK_LOG_LEVEL=none"],
                    "unset_environment": ["WINEDEBUG"]
                }
            }
        }"#;
        let store = Store::decode(data).unwrap();
        let profile = store
            .profiles
            .get(&"trainer".parse::<ProfileName>().unwrap())
            .unwrap();
        assert_eq!(profile.kind, ProfileKind::Proton);
        assert!(profile.run_options.detach);
    }
}
