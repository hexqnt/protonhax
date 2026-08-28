use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Cursor, Read, Write},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::Path,
    process::Command,
};

use crate::shell::split_env_assignment;

pub const ENV_FILE: &str = "env.bin";

const MAGIC: &[u8; 8] = b"PHENV\0\0\x01";
const MAX_ENV_FILE_SIZE: u64 = 16 * 1024 * 1024;

pub struct StoredEnv(Vec<(OsString, OsString)>);

impl StoredEnv {
    /// Снимает окружение процесса, применяет присваивания из команды запуска и исключает секреты.
    pub fn capture(overrides: &[String]) -> Self {
        let mut vars: BTreeMap<_, _> = env::vars_os()
            .filter(|(name, _)| should_store(name))
            .collect();

        for assignment in overrides {
            if let Some((name, value)) = split_env_assignment(assignment)
                && should_store(OsStr::new(name))
            {
                vars.insert(OsString::from(name), OsString::from(value));
            }
        }

        Self(vars.into_iter().collect())
    }

    /// Загружает бинарный снимок окружения с сохранением не-UTF-8 значений Unix.
    pub fn load(context_dir: &Path) -> io::Result<Self> {
        let path = context_dir.join(ENV_FILE);
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_ENV_FILE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "environment file is too large",
            ));
        }

        Self::decode(&fs::read(path)?)
    }

    pub fn write_to(&self, mut output: impl Write) -> io::Result<()> {
        if self.encoded_len()? > MAX_ENV_FILE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "environment snapshot is too large",
            ));
        }
        output.write_all(MAGIC)?;
        write_u32(&mut output, usize_to_u32(self.0.len())?)?;

        for (name, value) in &self.0 {
            let name = name.as_bytes();
            let value = value.as_bytes();
            write_u32(&mut output, usize_to_u32(name.len())?)?;
            write_u32(&mut output, usize_to_u32(value.len())?)?;
            output.write_all(name)?;
            output.write_all(value)?;
        }

        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&OsStr> {
        self.0
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value.as_os_str()))
    }

    /// Выводит снимок в однострочном диагностическом формате, скрывая секреты.
    pub fn write_display(&self, mut output: impl Write) -> io::Result<()> {
        for (name, value) in &self.0 {
            write!(output, "{}=", name.to_string_lossy())?;
            if is_sensitive_name(name) {
                writeln!(output, "<redacted>")?;
            } else {
                for character in value
                    .to_string_lossy()
                    .chars()
                    .flat_map(char::escape_default)
                {
                    write!(output, "{character}")?;
                }
                writeln!(output)?;
            }
        }
        Ok(())
    }

    /// Заменяет окружение дочернего процесса сохранённым снимком.
    pub fn configure(&self, command: &mut Command) {
        command.env_clear();
        command.envs(self.0.iter().map(|(name, value)| (name, value)));
    }

    fn encoded_len(&self) -> io::Result<u64> {
        self.0.iter().try_fold(12_u64, |total, (name, value)| {
            let entry_len = name
                .as_bytes()
                .len()
                .checked_add(value.as_bytes().len())
                .and_then(|length| length.checked_add(8))
                .and_then(|length| u64::try_from(length).ok())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "environment entry is too large",
                    )
                })?;
            total.checked_add(entry_len).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "environment snapshot is too large",
                )
            })
        })
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut input = Cursor::new(bytes);
        let mut magic = [0; MAGIC.len()];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported environment file format",
            ));
        }

        let count = read_u32(&mut input)? as usize;
        let mut vars = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let name_len = read_u32(&mut input)? as usize;
            let value_len = read_u32(&mut input)? as usize;
            let remaining = u64::try_from(bytes.len())
                .unwrap_or(u64::MAX)
                .saturating_sub(input.position());
            if name_len
                .checked_add(value_len)
                .and_then(|total_len| u64::try_from(total_len).ok())
                .is_none_or(|total_len| total_len > remaining)
            {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "environment entry exceeds the file size",
                ));
            }
            let name = read_bytes(&mut input, name_len)?;
            let value = read_bytes(&mut input, value_len)?;
            if name.is_empty() || name.contains(&0) || name.contains(&b'=') || value.contains(&0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "environment file contains an invalid variable",
                ));
            }
            let name = OsString::from_vec(name);
            if !should_store(&name) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "environment file contains a sensitive variable",
                ));
            }
            vars.push((name, OsString::from_vec(value)));
        }

        if input.position() != bytes.len() as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "environment file has trailing data",
            ));
        }

        Ok(Self(vars))
    }
}

pub(crate) fn is_sensitive_name(name: &OsStr) -> bool {
    let upper = name.to_string_lossy().to_ascii_uppercase();
    [
        "TOKEN",
        "PASSWORD",
        "SECRET",
        "PRIVATE_KEY",
        "ACCESS_KEY",
        "API_KEY",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

fn should_store(name: &OsStr) -> bool {
    !is_sensitive_name(name)
}

fn usize_to_u32(value: usize) -> io::Result<u32> {
    value.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "environment entry is too large",
        )
    })
}

fn write_u32(output: &mut impl Write, value: u32) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}

fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; size_of::<u32>()];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_bytes(input: &mut impl Read, len: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        os::unix::ffi::{OsStrExt, OsStringExt},
        process::Command,
    };

    use super::StoredEnv;

    fn roundtrip(vars: Vec<(OsString, OsString)>) -> StoredEnv {
        let original = StoredEnv(vars);
        let mut bytes = Vec::new();
        original.write_to(&mut bytes).unwrap();
        StoredEnv::decode(&bytes).unwrap()
    }

    #[test]
    fn preserves_newlines_and_non_utf8_values() {
        let env = roundtrip(vec![
            (OsString::from("MULTILINE"), OsString::from("one\ntwo")),
            (
                OsString::from("NON_UTF8"),
                OsString::from_vec(vec![b'a', 0xff, b'b']),
            ),
        ]);

        assert_eq!(env.get("MULTILINE"), Some(OsStr::new("one\ntwo")));
        assert_eq!(env.get("NON_UTF8").unwrap().as_bytes(), &[b'a', 0xff, b'b']);
    }

    #[test]
    fn applies_all_stored_variables_to_command() {
        let env = roundtrip(vec![
            (OsString::from("VALID"), OsString::from("one")),
            (OsString::from("QUOTED"), OsString::from("two words")),
        ]);
        let mut command = Command::new("true");
        env.configure(&mut command);

        let vars: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(StoredEnv::decode(b"PHENV\0\0\x01\x01").is_err());
    }

    #[test]
    fn capture_applies_overrides_and_filters_secrets() {
        let env = StoredEnv::capture(&[
            "PROTONHAX_TEST_VALUE=from-launch-command".to_owned(),
            "PROTONHAX_TEST_TOKEN=must-not-be-stored".to_owned(),
        ]);

        assert_eq!(
            env.get("PROTONHAX_TEST_VALUE"),
            Some(OsStr::new("from-launch-command"))
        );
        assert_eq!(env.get("PROTONHAX_TEST_TOKEN"), None);
    }

    #[test]
    fn diagnostic_output_redacts_sensitive_values() {
        let env = StoredEnv(vec![
            (OsString::from("VISIBLE"), OsString::from("one\ntwo")),
            (
                OsString::from("ACCESS_TOKEN"),
                OsString::from("must-not-be-printed"),
            ),
        ]);
        let mut output = Vec::new();
        env.write_display(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("VISIBLE=one\\ntwo"));
        assert!(output.contains("ACCESS_TOKEN=<redacted>"));
        assert!(!output.contains("must-not-be-printed"));
    }
}
