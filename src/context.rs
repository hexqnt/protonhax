pub use registry::{ContextDetail, RunningApp, active_apps, all_contexts};
pub use repair::{RepairSummary, repair_contexts};
pub use session::SessionId;
pub use storage::{
    EXE_FILE, PFX_FILE, PendingContext, STARTED_AT_FILE, read_started_at, read_stored_path,
};

use std::{fs, io, path::PathBuf, str::FromStr};

mod registry;
mod repair;
mod session;
mod storage;

fn typed_directories<T: FromStr>(
    entries: fs::ReadDir,
) -> impl Iterator<Item = io::Result<(T, PathBuf)>> {
    entries.filter_map(|entry| {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return Some(Err(error)),
        };
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            return None;
        }
        let identity = entry.file_name().to_str()?.parse().ok()?;
        Some(Ok((identity, entry.path())))
    })
}
