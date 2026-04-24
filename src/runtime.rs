use std::{
    env, fs,
    path::PathBuf,
    process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SECS_PER_MINUTE: u64 = 60;
const SECS_PER_HOUR: u64 = 60 * SECS_PER_MINUTE;
const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

/// Функция для получения пути к директории protonhax.
pub fn runtime_root() -> PathBuf {
    runtime_dir().join("protonhax")
}

fn runtime_dir() -> PathBuf {
    // Получаем XDG_RUNTIME_DIR или fallback на /run/user/<uid>.
    env::var_os("XDG_RUNTIME_DIR").map_or_else(
        || PathBuf::from(format!("/run/user/{}", current_uid())),
        PathBuf::from,
    )
}

fn current_uid() -> String {
    if let Ok(uid) = env::var("UID")
        && is_uid(&uid)
    {
        return uid;
    }

    if let Some(uid) = uid_from_proc_status() {
        return uid;
    }

    uid_from_id_command().unwrap_or_else(|| String::from("0"))
}

fn uid_from_proc_status() -> Option<String> {
    let content = fs::read_to_string("/proc/self/status").ok()?;
    let uid_line = content.lines().find(|line| line.starts_with("Uid:"))?;
    let uid = uid_line.split_whitespace().nth(1)?;
    if is_uid(uid) {
        Some(uid.to_string())
    } else {
        None
    }
}

fn uid_from_id_command() -> Option<String> {
    let output = process::Command::new("id").arg("-u").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    is_uid(&uid).then_some(uid)
}

fn is_uid(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn debug_enabled() -> bool {
    env::var_os("PROTONHAX_DEBUG").is_some()
}

pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

pub fn format_duration_ago(start_unix_secs: u64) -> String {
    let mut secs = unix_now_secs().saturating_sub(start_unix_secs);

    let days = secs / SECS_PER_DAY;
    secs %= SECS_PER_DAY;
    let hours = secs / SECS_PER_HOUR;
    secs %= SECS_PER_HOUR;
    let mins = secs / SECS_PER_MINUTE;
    let s = secs % SECS_PER_MINUTE;

    if days > 0 {
        if hours > 0 {
            format!("{days}d {hours}h ago")
        } else {
            format!("{days}d ago")
        }
    } else if hours > 0 {
        if mins > 0 {
            format!("{hours}h {mins}m ago")
        } else {
            format!("{hours}h ago")
        }
    } else if mins > 0 {
        if s > 0 {
            format!("{mins}m {s}s ago")
        } else {
            format!("{mins}m ago")
        }
    } else {
        format!("{s}s ago")
    }
}
