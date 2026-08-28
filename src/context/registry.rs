use std::{
    collections::{BTreeMap, btree_map::Entry},
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    app_id::AppId,
    env_store::StoredEnv,
    steam::{AppMeta, resolve_app_meta},
};

use super::{EXE_FILE, PFX_FILE, STARTED_AT_FILE, SessionId, read_stored_path};

#[derive(Clone, Copy)]
pub enum ContextDetail {
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
    pub fn recency(&self) -> (u64, u64, u32) {
        let (start_ticks, pid) = self.session_id.recency();
        (self.started_at.unwrap_or(0), start_ticks, pid)
    }
}

pub fn active_apps(runtime_root: &Path, detail: ContextDetail) -> io::Result<Vec<RunningApp>> {
    let mut latest = BTreeMap::<AppId, RunningApp>::new();
    for context in collect_contexts(runtime_root, detail, false)? {
        match latest.entry(context.appid) {
            Entry::Vacant(entry) => {
                entry.insert(context);
            }
            Entry::Occupied(mut entry) if context.recency() > entry.get().recency() => {
                entry.insert(context);
            }
            Entry::Occupied(_) => {}
        }
    }
    Ok(latest.into_values().collect())
}

pub fn all_contexts(runtime_root: &Path, detail: ContextDetail) -> io::Result<Vec<RunningApp>> {
    collect_contexts(runtime_root, detail, true)
}

fn collect_contexts(
    runtime_root: &Path,
    detail: ContextDetail,
    include_stale: bool,
) -> io::Result<Vec<RunningApp>> {
    let app_entries = match fs::read_dir(runtime_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut apps = Vec::new();
    for app_entry in app_entries {
        let app_entry = app_entry?;
        if !is_directory(&app_entry) {
            continue;
        }
        let Some(appid) = app_entry
            .file_name()
            .to_str()
            .and_then(|name| AppId::from_str(name).ok())
        else {
            continue;
        };
        let session_entries = match fs::read_dir(app_entry.path()) {
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
            let active = session_id.is_active();
            if active || include_stale {
                apps.push(read_context(
                    session_entry.path(),
                    appid,
                    session_id,
                    active,
                    detail,
                ));
            }
        }
    }

    apps.sort_by_key(|app| (app.appid, app.recency()));
    Ok(apps)
}

fn read_context(
    path: PathBuf,
    appid: AppId,
    session_id: SessionId,
    active: bool,
    detail: ContextDetail,
) -> RunningApp {
    let meta = StoredEnv::load(&path).map_or_else(
        |_| AppMeta::default(),
        |environment| resolve_app_meta(&environment, appid),
    );
    let (prefix_path, proton_path, started_at) = match detail {
        ContextDetail::Summary => (None, None, None),
        ContextDetail::Full => (
            read_stored_path(path.join(PFX_FILE)).ok(),
            read_stored_path(path.join(EXE_FILE)).ok(),
            read_started_at(&path),
        ),
    };

    RunningApp {
        appid,
        session_id,
        path,
        active,
        name: meta.name,
        install_path: meta.install_path,
        compatdata_path: meta.compatdata_path,
        prefix_path,
        proton_path,
        started_at,
    }
}

fn is_directory(entry: &fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|file_type| file_type.is_dir())
}

fn read_started_at(context_dir: &Path) -> Option<u64> {
    fs::read_to_string(context_dir.join(STARTED_AT_FILE))
        .ok()?
        .parse()
        .ok()
}
