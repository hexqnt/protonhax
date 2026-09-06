use std::{
    env, fs, io,
    path::PathBuf,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

const SECS_PER_MINUTE: u64 = 60;
const SECS_PER_HOUR: u64 = 60 * SECS_PER_MINUTE;
const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

/// Возвращает абсолютный каталог runtime, не доверяя относительному `XDG_RUNTIME_DIR`.
pub fn runtime_root() -> io::Result<PathBuf> {
    let runtime_dir = match env::var_os("XDG_RUNTIME_DIR") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(format!("/run/user/{}", current_uid()?)),
    };
    if !runtime_dir.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "XDG_RUNTIME_DIR must be an absolute path",
        ));
    }

    Ok(runtime_dir.join("protonhax"))
}

pub fn debug_enabled() -> bool {
    env::var_os("PROTONHAX_DEBUG").is_some()
}

pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn format_duration_ago(start_unix_secs: u64) -> String {
    let mut secs = unix_now_secs().saturating_sub(start_unix_secs);

    let days = secs / SECS_PER_DAY;
    secs %= SECS_PER_DAY;
    let hours = secs / SECS_PER_HOUR;
    secs %= SECS_PER_HOUR;
    let minutes = secs / SECS_PER_MINUTE;
    let seconds = secs % SECS_PER_MINUTE;

    match (days, hours, minutes, seconds) {
        (days, hours, _, _) if days > 0 && hours > 0 => format!("{days}d {hours}h ago"),
        (days, _, _, _) if days > 0 => format!("{days}d ago"),
        (_, hours, minutes, _) if hours > 0 && minutes > 0 => {
            format!("{hours}h {minutes}m ago")
        }
        (_, hours, _, _) if hours > 0 => format!("{hours}h ago"),
        (_, _, minutes, seconds) if minutes > 0 && seconds > 0 => {
            format!("{minutes}m {seconds}s ago")
        }
        (_, _, minutes, _) if minutes > 0 => format!("{minutes}m ago"),
        _ => format!("{seconds}s ago"),
    }
}

pub(crate) fn current_uid() -> io::Result<u32> {
    if let Some(uid) = uid_from_proc_status() {
        return Ok(uid);
    }

    let output = process::Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(io::Error::other("failed to determine current uid"));
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn uid_from_proc_status() -> Option<u32> {
    let content = fs::read_to_string("/proc/self/status").ok()?;
    content
        .lines()
        .find(|line| line.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::format_duration_ago;

    #[test]
    fn future_timestamp_is_zero_seconds_ago() {
        assert_eq!(format_duration_ago(u64::MAX), "0s ago");
    }
}
