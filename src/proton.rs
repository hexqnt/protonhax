use std::{
    env,
    ffi::OsStr,
    fmt, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct ProtonInvocation {
    executable: PathBuf,
}

impl ProtonInvocation {
    /// Находит именно исполняемый файл `proton`, а не аргумент, лишь содержащий это слово.
    pub fn parse(
        command: &[String],
        search_path: Option<&OsStr>,
    ) -> Result<Self, ProtonInvocationError> {
        command
            .iter()
            .filter(|argument| {
                Path::new(argument)
                    .file_name()
                    .is_some_and(|name| name == "proton")
            })
            .find_map(|argument| {
                resolve_executable(argument, search_path).map(|executable| Self { executable })
            })
            .ok_or(ProtonInvocationError)
    }

    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }
}

#[derive(Debug)]
pub struct ProtonInvocationError;

impl fmt::Display for ProtonInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("executable Proton path not found in command")
    }
}

fn resolve_executable(argument: &str, search_path: Option<&OsStr>) -> Option<PathBuf> {
    let path = Path::new(argument);
    if path.components().count() > 1 {
        return is_executable_file(path).then(|| path.to_path_buf());
    }

    let inherited_path = env::var_os("PATH");
    env::split_paths(search_path.or(inherited_path.as_deref())?)
        .map(|directory| directory.join(path))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &std::path::Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::ProtonInvocation;

    #[test]
    fn ignores_non_executable_arguments_containing_proton() {
        assert!(
            ProtonInvocation::parse(
                &["wrapper".to_owned(), "/tmp/proton-trainer.exe".to_owned()],
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_an_executable_named_proton() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("protonhax-test-{}-{nonce}", process::id()));
        fs::create_dir(&directory).unwrap();
        let proton = directory.join("proton");
        fs::write(&proton, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&proton, fs::Permissions::from_mode(0o700)).unwrap();

        let parsed = ProtonInvocation::parse(
            &["wrapper".to_owned(), proton.to_string_lossy().into_owned()],
            None,
        )
        .unwrap();
        assert_eq!(parsed.executable(), proton);
        let parsed_from_path =
            ProtonInvocation::parse(&["proton".to_owned()], Some(directory.as_os_str())).unwrap();
        assert_eq!(parsed_from_path.executable(), proton);

        fs::remove_file(proton).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
