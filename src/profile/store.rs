use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufWriter, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, Serializer};

use crate::{
    launch::{EnvironmentChanges, RunOptions},
    runtime::current_uid,
};

use super::{Profile, ProfileName, view::ProfileView};

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
        let dto: StoreDto = serde_json::from_slice(bytes).map_err(invalid_data)?;
        if dto.version != CONFIG_VERSION {
            return Err(invalid_data("unsupported profile configuration version"));
        }

        let profiles = dto
            .profiles
            .into_iter()
            .map(|(name, profile)| Ok((name.parse().map_err(invalid_data)?, profile.try_into()?)))
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
            let mut file = BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temporary)?,
            );
            serde_json::to_writer_pretty(&mut file, &StoreView::from(self))
                .map_err(io::Error::other)?;
            writeln!(file)?;
            file.flush()?;
            file.get_ref().sync_all()?;
            fs::rename(&temporary, destination)?;
            fs::File::open(root)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

#[derive(Deserialize)]
struct StoreDto {
    version: u32,
    profiles: BTreeMap<String, ProfileDto>,
}

#[derive(Serialize)]
struct StoreView<'a> {
    version: u32,
    #[serde(serialize_with = "serialize_profiles")]
    profiles: &'a BTreeMap<ProfileName, Profile>,
}

impl<'a> From<&'a Store> for StoreView<'a> {
    fn from(store: &'a Store) -> Self {
        Self {
            version: CONFIG_VERSION,
            profiles: &store.profiles,
        }
    }
}

fn serialize_profiles<S: Serializer>(
    profiles: &BTreeMap<ProfileName, Profile>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_map(
        profiles
            .iter()
            .map(|(name, profile)| (name.as_str(), ProfileView::new(profile, false))),
    )
}

#[derive(Deserialize)]
struct ProfileDto {
    target: String,
    kind: String,
    command: Vec<String>,
    detach: bool,
    cwd: Option<String>,
    environment: Vec<String>,
    unset_environment: Vec<String>,
}

impl TryFrom<ProfileDto> for Profile {
    type Error = io::Error;

    fn try_from(dto: ProfileDto) -> Result<Self, Self::Error> {
        Self::new(
            dto.target.parse().map_err(invalid_data)?,
            dto.kind.parse()?,
            dto.command,
            RunOptions {
                detach: dto.detach,
                cwd: dto
                    .cwd
                    .map(|cwd| cwd.parse().map_err(invalid_data))
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
        .map(|value| value.parse().map_err(invalid_data))
        .collect()
}

fn invalid_data(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::io;

    use serde_json::{Value, json};

    use super::{Store, StoreView};
    use crate::profile::{ProfileKind, ProfileName, profile_json};

    const LEGACY_CONFIG: &[u8] = br#"{
        "version": 1,
        "profiles": {
            "trainer": {
                "target": "latest",
                "kind": "proton",
                "command": ["/tmp/trainer.exe", "--message", "quoted \"value\""],
                "detach": true,
                "cwd": "exe-dir",
                "environment": ["DXVK_LOG_LEVEL=none", "TRAINER_TOKEN=secret=value"],
                "unset_environment": ["WINEDEBUG"]
            }
        }
    }"#;

    #[test]
    fn legacy_configuration_roundtrips_without_changing_the_contract() {
        let store = Store::decode(LEGACY_CONFIG).unwrap();
        let profile = store
            .profiles
            .get(&"trainer".parse::<ProfileName>().unwrap())
            .unwrap();
        assert_eq!(profile.kind, ProfileKind::Proton);
        assert!(profile.run_options.detach);

        let encoded = serde_json::to_vec(&StoreView::from(&store)).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&encoded).unwrap(),
            serde_json::from_slice::<Value>(LEGACY_CONFIG).unwrap()
        );
        assert_eq!(Store::decode(&encoded).unwrap().profiles, store.profiles);
    }

    #[test]
    fn listing_redacts_secrets_without_changing_the_persisted_profile() {
        let store = Store::decode(LEGACY_CONFIG).unwrap();
        let persisted = serde_json::to_vec(&StoreView::from(&store)).unwrap();
        let (name, profile) = store.profiles.first_key_value().unwrap();
        let listed = profile_json(name, profile).unwrap();
        assert_eq!(listed["name"], "trainer");
        assert_eq!(
            listed["environment"],
            json!(["DXVK_LOG_LEVEL=none", "TRAINER_TOKEN=<redacted>"])
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&persisted).unwrap()["profiles"]["trainer"]["environment"],
            json!(["DXVK_LOG_LEVEL=none", "TRAINER_TOKEN=secret=value"])
        );
        assert_eq!(
            serde_json::to_vec(&StoreView::from(&store)).unwrap(),
            persisted
        );
    }

    #[test]
    fn invalid_configuration_keeps_typed_errors() {
        let cases = [
            (
                "target",
                json!("4294967296"),
                io::ErrorKind::InvalidData,
                "appid is out of range:",
            ),
            (
                "kind",
                json!("PROTON"),
                io::ErrorKind::InvalidData,
                "invalid profile kind",
            ),
            (
                "command",
                json!([]),
                io::ErrorKind::InvalidInput,
                "profile command must not be empty",
            ),
            (
                "cwd",
                json!(""),
                io::ErrorKind::InvalidData,
                "working directory must not be empty",
            ),
            (
                "environment",
                json!(["MISSING_EQUALS"]),
                io::ErrorKind::InvalidData,
                "environment override must have the NAME=VALUE form",
            ),
            (
                "environment",
                json!(["BAD-NAME=value"]),
                io::ErrorKind::InvalidData,
                "invalid environment variable name",
            ),
            (
                "unset_environment",
                json!(["BAD-NAME"]),
                io::ErrorKind::InvalidData,
                "invalid environment variable name",
            ),
            (
                "kind",
                json!("native"),
                io::ErrorKind::InvalidInput,
                "--detach and --cwd are only supported by proton profiles",
            ),
        ];
        for (field, value, kind, message) in cases {
            let mut config: Value = serde_json::from_slice(LEGACY_CONFIG).unwrap();
            config["profiles"]["trainer"][field] = value;
            let error = Store::decode(&serde_json::to_vec(&config).unwrap())
                .err()
                .unwrap();
            assert_eq!(error.kind(), kind, "{field}");
            assert!(error.to_string().starts_with(message), "{field}: {error}");
        }

        let mut config: Value = serde_json::from_slice(LEGACY_CONFIG).unwrap();
        config["version"] = 2.into();
        let error = Store::decode(&serde_json::to_vec(&config).unwrap())
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "unsupported profile configuration version"
        );

        let profile = config["profiles"]["trainer"].take();
        config["version"] = 1.into();
        config["profiles"] = json!({ "../trainer": profile });
        let error = Store::decode(&serde_json::to_vec(&config).unwrap())
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "profile name may contain only ASCII letters, digits, '_', '-' and '.'"
        );
    }
}
