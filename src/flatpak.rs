use std::{env, fs, path::PathBuf};

const STEAM_FLATPAK_ID: &str = "com.valvesoftware.Steam";

#[derive(Debug, Eq, PartialEq)]
pub enum FlatpakStatus {
    InsideSteam,
    InsideOther(String),
    InsideUnknown,
    HostInstallation(PathBuf),
    NotDetected,
}

pub fn detect() -> FlatpakStatus {
    if let Some(app_id) = env::var_os("FLATPAK_ID") {
        let app_id = app_id.to_string_lossy();
        return if app_id == STEAM_FLATPAK_ID {
            FlatpakStatus::InsideSteam
        } else {
            FlatpakStatus::InsideOther(app_id.into_owned())
        };
    }
    if fs::exists("/.flatpak-info").unwrap_or(false) {
        return FlatpakStatus::InsideUnknown;
    }

    let installation = env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".var/app").join(STEAM_FLATPAK_ID));
    match installation {
        Some(path) if path.is_dir() => FlatpakStatus::HostInstallation(path),
        _ => FlatpakStatus::NotDetected,
    }
}

#[cfg(test)]
mod tests {
    use super::STEAM_FLATPAK_ID;

    #[test]
    fn steam_application_id_is_stable() {
        assert_eq!(STEAM_FLATPAK_ID, "com.valvesoftware.Steam");
    }
}
