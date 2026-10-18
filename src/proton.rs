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

    pub fn executable(&self) -> &Path {
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
    if path.components().nth(1).is_some() {
        return is_executable_file(path).then(|| path.to_path_buf());
    }

    let inherited_path = env::var_os("PATH");
    env::split_paths(search_path.or(inherited_path.as_deref())?)
        .map(|directory| directory.join(path))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, os::unix::fs::PermissionsExt};

    use crate::test_support::TestDirectory;

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
        let directory = TestDirectory::new().unwrap();
        let proton = directory.path().join("proton");
        fs::write(&proton, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&proton, fs::Permissions::from_mode(0o700)).unwrap();

        let parsed = ProtonInvocation::parse(
            &["wrapper".to_owned(), proton.to_string_lossy().into_owned()],
            None,
        )
        .unwrap();
        assert_eq!(parsed.executable(), proton);
        let parsed_from_path =
            ProtonInvocation::parse(&["proton".to_owned()], Some(directory.path().as_os_str()))
                .unwrap();
        assert_eq!(parsed_from_path.executable(), proton);
    }

    #[test]
    fn skips_non_executable_candidates_in_argv_and_search_path() {
        let directory = TestDirectory::new().unwrap();
        let blocked = directory.path().join("blocked");
        let available = directory.path().join("available");
        fs::create_dir(&blocked).unwrap();
        fs::create_dir(&available).unwrap();
        let blocked_proton = blocked.join("proton");
        fs::write(&blocked_proton, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&blocked_proton, fs::Permissions::from_mode(0o600)).unwrap();
        let proton = available.join("proton");
        fs::write(&proton, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&proton, fs::Permissions::from_mode(0o700)).unwrap();
        let search_path = env::join_paths([&blocked, &available]).unwrap();

        let parsed = ProtonInvocation::parse(
            &[
                blocked_proton.to_str().unwrap().to_owned(),
                "proton".to_owned(),
                blocked_proton.to_str().unwrap().to_owned(),
            ],
            Some(&search_path),
        )
        .unwrap();
        assert_eq!(parsed.executable(), proton);

        assert!(
            ProtonInvocation::parse(&["proton".to_owned()], Some(blocked.as_os_str())).is_err()
        );
    }
}
