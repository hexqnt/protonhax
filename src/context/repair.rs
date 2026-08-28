use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    str::FromStr,
};

use crate::{app_id::AppId, env_store::ENV_FILE, runtime::current_uid};

use super::{EXE_FILE, PFX_FILE, STARTED_AT_FILE, SessionId};

#[derive(Clone, Copy)]
enum PathKind {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RepairSummary {
    pub stale_contexts_removed: usize,
    pub permissions_fixed: usize,
}

pub fn repair_contexts(runtime_root: &Path) -> io::Result<RepairSummary> {
    let mut summary = RepairSummary::default();
    match set_mode_if_needed(runtime_root, 0o700, PathKind::Directory) {
        Ok(changed) => summary.permissions_fixed += usize::from(changed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(summary),
        Err(error) => return Err(error),
    }

    for app_entry in fs::read_dir(runtime_root)? {
        let app_entry = app_entry?;
        if !is_directory(&app_entry)
            || app_entry
                .file_name()
                .to_str()
                .and_then(|name| AppId::from_str(name).ok())
                .is_none()
        {
            continue;
        }
        let app_dir = app_entry.path();
        summary.permissions_fixed +=
            usize::from(set_mode_if_needed(&app_dir, 0o700, PathKind::Directory)?);

        let session_entries = match fs::read_dir(&app_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for session_entry in session_entries {
            let session_entry = session_entry?;
            if !is_directory(&session_entry) {
                continue;
            }
            let Some(session_id) = session_entry
                .file_name()
                .to_str()
                .and_then(|name| SessionId::from_str(name).ok())
            else {
                continue;
            };
            let session_dir = session_entry.path();
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
            )?);
            for file in [EXE_FILE, PFX_FILE, STARTED_AT_FILE, ENV_FILE] {
                match set_mode_if_needed(&session_dir.join(file), 0o600, PathKind::File) {
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

fn set_mode_if_needed(path: &Path, expected: u32, kind: PathKind) -> io::Result<bool> {
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
    if metadata.uid() != current_uid()? {
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

fn is_directory(entry: &fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|file_type| file_type.is_dir())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::repair_contexts;

    #[test]
    fn repair_removes_only_typed_stale_sessions() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("protonhax-repair-{}-{nonce}", process::id()));
        let app_dir = root.join("570");
        let stale = app_dir.join("4294967295-1");
        fs::create_dir_all(&stale).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir(app_dir.join("not-a-session")).unwrap();

        let summary = repair_contexts(&root).unwrap();
        assert_eq!(summary.stale_contexts_removed, 1);
        assert!(!stale.exists());
        assert!(app_dir.join("not-a-session").exists());
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );

        fs::remove_dir_all(root).unwrap();
    }
}
