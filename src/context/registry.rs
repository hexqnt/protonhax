use std::{
    collections::{BTreeMap, btree_map::Entry},
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    app_id::AppId,
    env_store::StoredEnv,
    steam::{AppMeta, resolve_app_meta},
};

use super::{EXE_FILE, PFX_FILE, SessionId, read_started_at, read_stored_path, typed_directories};

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContextDetail {
    Identity,
    Summary,
    Full,
}

pub struct RunningApp {
    pub appid: AppId,
    pub session_id: SessionId,
    pub path: PathBuf,
    pub active: bool,
    pub name: Option<String>,
    pub install_path: Option<PathBuf>,
    pub compatdata_path: Option<PathBuf>,
    pub prefix_path: Option<PathBuf>,
    pub proton_path: Option<PathBuf>,
    pub started_at: Option<u64>,
}

impl RunningApp {
    pub fn recency(&self) -> (u64, u32) {
        self.session_id.recency()
    }
}

struct ContextEntry {
    appid: AppId,
    session_id: SessionId,
    path: PathBuf,
    active: bool,
}

impl ContextEntry {
    fn load(self, detail: ContextDetail) -> RunningApp {
        let meta = if detail >= ContextDetail::Summary {
            StoredEnv::load(&self.path).map_or_else(
                |_| AppMeta::default(),
                |environment| resolve_app_meta(&environment, self.appid),
            )
        } else {
            AppMeta::default()
        };
        let (prefix_path, proton_path, started_at) = match detail {
            ContextDetail::Identity | ContextDetail::Summary => (None, None, None),
            ContextDetail::Full => (
                read_stored_path(self.path.join(PFX_FILE)).ok(),
                read_stored_path(self.path.join(EXE_FILE)).ok(),
                read_started_at(&self.path),
            ),
        };

        RunningApp {
            appid: self.appid,
            session_id: self.session_id,
            path: self.path,
            active: self.active,
            name: meta.name,
            install_path: meta.install_path,
            compatdata_path: meta.compatdata_path,
            prefix_path,
            proton_path,
            started_at,
        }
    }
}

pub fn active_apps(runtime_root: &Path, detail: ContextDetail) -> io::Result<Vec<RunningApp>> {
    let mut latest = BTreeMap::<AppId, ContextEntry>::new();
    visit_contexts(runtime_root, |context| {
        if !context.active {
            return;
        }
        match latest.entry(context.appid) {
            Entry::Vacant(entry) => {
                entry.insert(context);
            }
            Entry::Occupied(mut entry)
                if context.session_id.recency() > entry.get().session_id.recency() =>
            {
                entry.insert(context);
            }
            Entry::Occupied(_) => {}
        }
    })?;
    Ok(latest
        .into_values()
        .map(|context| context.load(detail))
        .collect())
}

pub fn all_contexts(runtime_root: &Path, detail: ContextDetail) -> io::Result<Vec<RunningApp>> {
    let mut contexts = Vec::new();
    visit_contexts(runtime_root, |context| contexts.push(context.load(detail)))?;
    contexts.sort_by_key(|app| (app.appid, app.session_id.recency()));
    Ok(contexts)
}

fn visit_contexts(runtime_root: &Path, mut visit: impl FnMut(ContextEntry)) -> io::Result<()> {
    let app_entries = match fs::read_dir(runtime_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    for app in typed_directories::<AppId>(app_entries) {
        let (appid, app_dir) = app?;
        let session_entries = match fs::read_dir(app_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };

        for session in typed_directories::<SessionId>(session_entries) {
            let (session_id, path) = session?;
            let active = session_id.is_active();
            visit(ContextEntry {
                appid,
                session_id,
                path,
                active,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::symlink,
        process::{self, Command},
    };

    use crate::{
        context::SessionId,
        env_store::{ENV_FILE, StoredEnv},
        test_support::TestDirectory,
    };

    use super::{ContextDetail, active_apps, all_contexts};

    #[test]
    fn registry_filters_entries_and_selects_latest_active_session_per_app() {
        let directory = TestDirectory::new().unwrap();
        let root = directory.path();
        let current = SessionId::for_pid(process::id()).unwrap();
        let mut child = Command::new("sleep").arg("5").spawn().unwrap();
        let latest = SessionId::for_pid(child.id()).unwrap();
        assert!(latest.recency() > current.recency());
        for relative in [
            format!("570/{current}"),
            format!("570/{latest}"),
            "570/4294967295-1".to_owned(),
            "570/not-a-session".to_owned(),
            "2/4294967295-1".to_owned(),
            format!("invalid/{current}"),
        ] {
            fs::create_dir_all(root.join(relative)).unwrap();
        }
        fs::write(root.join("570/1-2"), "file").unwrap();
        symlink(root.join("570"), root.join("730")).unwrap();
        symlink(
            root.join("570").join(current.to_string()),
            root.join("570/1-3"),
        )
        .unwrap();

        let all = all_contexts(root, ContextDetail::Identity).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].appid.to_string(), "2");
        assert_eq!(all.iter().filter(|app| app.active).count(), 2);
        assert!(all.windows(2).all(|pair| {
            (pair[0].appid, pair[0].recency()) <= (pair[1].appid, pair[1].recency())
        }));
        let active = active_apps(root, ContextDetail::Identity).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, latest);

        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn context_detail_preserves_lazy_metadata_loading() {
        let directory = TestDirectory::new().unwrap();
        let root = directory.path().join("runtime");
        let session = SessionId::for_pid(process::id()).unwrap();
        let context = root.join("570").join(session.to_string());
        let steamapps = directory.path().join("steamapps");
        let compat = steamapps.join("compatdata/570");
        fs::create_dir_all(&context).unwrap();
        fs::create_dir_all(&compat).unwrap();
        fs::write(
            steamapps.join("appmanifest_570.acf"),
            "\"name\" \"Test game\"\n\"installdir\" \"Test directory\"",
        )
        .unwrap();
        StoredEnv::capture(&[format!("STEAM_COMPAT_DATA_PATH={}", compat.display())])
            .write_to(fs::File::create(context.join(ENV_FILE)).unwrap())
            .unwrap();
        fs::write(context.join("exe"), "/proton").unwrap();
        fs::write(context.join("pfx"), "/prefix").unwrap();
        fs::write(context.join("started_at"), "42").unwrap();

        for detail in [
            ContextDetail::Identity,
            ContextDetail::Summary,
            ContextDetail::Full,
        ] {
            let mut apps = active_apps(&root, detail).unwrap();
            let app = apps.pop().unwrap();
            assert_eq!(
                app.name.as_deref(),
                (detail >= ContextDetail::Summary).then_some("Test game")
            );
            assert_eq!(
                app.compatdata_path.as_deref(),
                (detail >= ContextDetail::Summary).then_some(compat.as_path())
            );
            assert_eq!(
                app.started_at,
                (detail == ContextDetail::Full).then_some(42)
            );
            assert_eq!(
                app.prefix_path.as_deref(),
                (detail == ContextDetail::Full).then_some(std::path::Path::new("/prefix"))
            );
            assert_eq!(
                app.proton_path.as_deref(),
                (detail == ContextDetail::Full).then_some(std::path::Path::new("/proton"))
            );
        }
    }

    #[test]
    fn missing_runtime_is_empty_but_other_directory_errors_propagate() {
        let directory = TestDirectory::new().unwrap();
        let root = directory.path().join("runtime");
        assert!(
            all_contexts(&root, ContextDetail::Identity)
                .unwrap()
                .is_empty()
        );
        assert!(
            active_apps(&root, ContextDetail::Identity)
                .unwrap()
                .is_empty()
        );
        fs::write(&root, "file").unwrap();
        assert!(all_contexts(&root, ContextDetail::Identity).is_err());
        assert!(active_apps(&root, ContextDetail::Identity).is_err());
    }
}
