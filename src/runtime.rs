use std::{
    env, fs,
    path::PathBuf,
    process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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
        && uid.chars().all(|c| c.is_ascii_digit())
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
    if uid.chars().all(|c| c.is_ascii_digit()) {
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
    if uid.chars().all(|c| c.is_ascii_digit()) {
        Some(uid)
    } else {
        None
    }
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

    let days = secs / 86_400;
    secs %= 86_400;
    let hours = secs / 3_600;
    secs %= 3_600;
    let mins = secs / 60;
    let s = secs % 60;

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
