use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

use serde::Serialize;

use crate::{app_id::AppId, env_store::ENV_FILE, runtime::current_uid};

use super::{EXE_FILE, PFX_FILE, STARTED_AT_FILE, SessionId, typed_directories};

#[derive(Clone, Copy)]
enum PathKind {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RepairSummary {
    pub stale_contexts_removed: usize,
    pub permissions_fixed: usize,
}

pub fn repair_contexts(runtime_root: &Path) -> io::Result<RepairSummary> {
    let mut summary = RepairSummary::default();
    let uid = current_uid()?;
    match set_mode_if_needed(runtime_root, 0o700, PathKind::Directory, uid) {
        Ok(changed) => summary.permissions_fixed += usize::from(changed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(summary),
        Err(error) => return Err(error),
    }

    for app in typed_directories::<AppId>(fs::read_dir(runtime_root)?) {
        let (_, app_dir) = app?;
        summary.permissions_fixed += usize::from(set_mode_if_needed(
            &app_dir,
            0o700,
            PathKind::Directory,
            uid,
        )?);

        let session_entries = match fs::read_dir(&app_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for session in typed_directories::<SessionId>(session_entries) {
            let (session_id, session_dir) = session?;
            if !session_id.is_active() {
                match fs::remove_dir_all(&session_dir) {
                    Ok(()) => summary.stale_contexts_removed += 1,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                continue;
            }

            summary.permissions_fixed += usize::from(set_mode_if_needed(
                &session_dir,
                0o700,
                PathKind::Directory,
                uid,
            )?);
            for file in [EXE_FILE, PFX_FILE, STARTED_AT_FILE, ENV_FILE] {
                match set_mode_if_needed(&session_dir.join(file), 0o600, PathKind::File, uid) {
                    Ok(changed) => summary.permissions_fixed += usize::from(changed),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        let _ = fs::remove_dir(&app_dir);
    }

    Ok(summary)
}

fn set_mode_if_needed(path: &Path, expected: u32, kind: PathKind, uid: u32) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "refusing to change permissions of symlink: {}",
                path.display()
            ),
        ));
    }
    if metadata.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("runtime path is owned by another user: {}", path.display()),
        ));
    }
    let type_matches = match kind {
        PathKind::Directory => metadata.is_dir(),
        PathKind::File => metadata.is_file(),
    };
    if !type_matches {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("runtime path has an unexpected type: {}", path.display()),
        ));
    }
    if metadata.permissions().mode() & 0o777 == expected {
        return Ok(false);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(expected))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        process,
    };

    use super::repair_contexts;
    use crate::{context::SessionId, test_support::TestDirectory};

    #[test]
    fn repair_removes_only_typed_stale_sessions() {
        let directory = TestDirectory::new().unwrap();
        let root = directory.path();
        let app_dir = root.join("570");
        let stale = app_dir.join("4294967295-1");
        fs::create_dir_all(&stale).unwrap();
        fs::set_permissions(root, fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir(app_dir.join("not-a-session")).unwrap();

        let summary = repair_contexts(root).unwrap();
        assert_eq!(summary.stale_contexts_removed, 1);
        assert!(!stale.exists());
        assert!(app_dir.join("not-a-session").exists());
        assert_eq!(
            fs::metadata(root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn repair_refuses_symlink_files_in_active_sessions() {
        let directory = TestDirectory::new().unwrap();
        let root = directory.path().join("runtime");
        let session = SessionId::for_pid(process::id()).unwrap();
        let context = root.join("570").join(session.to_string());
        fs::create_dir_all(&context).unwrap();
        let target = directory.path().join("external");
        fs::write(&target, "external").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&target, context.join("exe")).unwrap();

        let error = repair_contexts(&root).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error
                .to_string()
                .starts_with("refusing to change permissions of symlink:")
        );
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert!(context.is_dir());
    }
}
