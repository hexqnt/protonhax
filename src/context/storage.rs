use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    app_id::AppId,
    env_store::{ENV_FILE, StoredEnv},
    runtime::current_uid,
};

use super::SessionId;

pub const EXE_FILE: &str = "exe";
pub const PFX_FILE: &str = "pfx";
pub const STARTED_AT_FILE: &str = "started_at";

const MAX_STORED_PATH_SIZE: u64 = 64 * 1024;

pub struct PendingContext {
    path: PathBuf,
    app_dir: PathBuf,
}

impl PendingContext {
    pub fn create(
        runtime_root: &Path,
        appid: AppId,
        proton_path: &Path,
        prefix_path: &Path,
        environment: &StoredEnv,
        started_at: u64,
    ) -> io::Result<Self> {
        let uid = current_uid()?;
        ensure_private_dir(runtime_root, uid)?;
        let app_dir = runtime_root.join(appid.to_string());
        ensure_private_dir(&app_dir, uid)?;

        let identity = SessionId::for_pid(process::id())?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = app_dir.join(format!(".pending-{identity}-{nonce}"));
        create_private_dir(&path)?;

        if let Err(error) =
            write_context_files(&path, proton_path, prefix_path, environment, started_at)
        {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }

        Ok(Self { path, app_dir })
    }

    pub fn publish(mut self, owner_pid: u32) -> io::Result<PublishedContext> {
        let session_id = SessionId::for_pid(owner_pid)?;
        let final_path = self.app_dir.join(session_id.to_string());
        fs::rename(&self.path, &final_path)?;
        let _ = sync_directory(&self.app_dir);
        let app_dir = std::mem::take(&mut self.app_dir);

        Ok(PublishedContext {
            path: final_path,
            app_dir,
        })
    }
}

impl Drop for PendingContext {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub struct PublishedContext {
    path: PathBuf,
    app_dir: PathBuf,
}

impl Drop for PublishedContext {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_dir(&self.app_dir);
    }
}

pub fn read_stored_path(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_STORED_PATH_SIZE + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_STORED_PATH_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stored path is too large",
        ));
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stored path is invalid",
        ));
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

fn write_context_files(
    path: &Path,
    proton_path: &Path,
    prefix_path: &Path,
    environment: &StoredEnv,
    started_at: u64,
) -> io::Result<()> {
    write_private(&path.join(EXE_FILE), proton_path.as_os_str().as_bytes())?;
    write_private(&path.join(PFX_FILE), prefix_path.as_os_str().as_bytes())?;
    write_private(
        &path.join(STARTED_AT_FILE),
        started_at.to_string().as_bytes(),
    )?;
    let mut env_file = create_private_file(&path.join(ENV_FILE))?;
    environment.write_to(&mut env_file)?;
    env_file.sync_all()?;
    sync_directory(path)
}

fn ensure_private_dir(path: &Path, uid: u32) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "runtime path is not a regular directory: {}",
                        path.display()
                    ),
                ));
            }
            if metadata.uid() != uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "runtime directory is owned by another user: {}",
                        path.display()
                    ),
                ));
            }
            if metadata.permissions().mode() & 0o777 == 0o700 {
                Ok(())
            } else {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => create_private_dir(path),
        Err(error) => Err(error),
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}

fn create_private_file(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

fn write_private(path: &Path, value: &[u8]) -> io::Result<()> {
    let mut file = create_private_file(path)?;
    file.write_all(value)?;
    file.sync_all()
}

fn sync_directory(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::Path,
        process::{self, Command},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        app_id::AppId,
        env_store::{ENV_FILE, StoredEnv},
    };

    use super::PendingContext;

    #[test]
    fn context_is_published_atomically_and_removed_by_its_guard() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("protonhax-{}-{nonce}", process::id()));
        let appid = "1217060".parse::<AppId>().unwrap();
        let environment = StoredEnv::capture(&[]);
        let pending = PendingContext::create(
            &root,
            appid,
            Path::new("/bin/true"),
            Path::new("/tmp/pfx"),
            &environment,
            42,
        )
        .unwrap();
        let app_dir = root.join(appid.to_string());
        assert!(fs::read_dir(&app_dir).unwrap().all(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        }));

        let mut child = Command::new("sleep").arg("5").spawn().unwrap();
        let published = pending.publish(child.id()).unwrap();
        assert!(published.path.is_dir());
        assert_eq!(
            fs::metadata(&published.path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(published.path.join(ENV_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        drop(published);
        assert!(!app_dir.exists());
        child.kill().unwrap();
        child.wait().unwrap();
        fs::remove_dir(root).unwrap();
    }
}
