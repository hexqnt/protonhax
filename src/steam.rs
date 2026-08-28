use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{app_id::AppId, env_store::StoredEnv};

pub const STEAM_APP_ID_ENV: &str = "SteamAppId";
pub const STEAM_COMPAT_DATA_PATH_ENV: &str = "STEAM_COMPAT_DATA_PATH";

#[derive(Default)]
pub struct AppMeta {
    pub name: Option<String>,
    pub install_path: Option<PathBuf>,
    pub compatdata_path: Option<PathBuf>,
}

#[derive(Default)]
struct ManifestInfo {
    name: Option<String>,
    installdir: Option<String>,
}

pub fn resolve_app_meta(environment: &StoredEnv, appid: AppId) -> AppMeta {
    let Some(compat_data) = environment.get("STEAM_COMPAT_DATA_PATH") else {
        return AppMeta::default();
    };

    let compatdata_path = PathBuf::from(compat_data);
    let appid = appid.to_string();
    let direct_steamapps = steamapps_path_from_compat(&compatdata_path, &appid);
    let manifest = direct_steamapps
        .and_then(|steamapps| read_manifest(steamapps, &appid))
        .or_else(|| {
            let steam_root = environment.get("STEAM_COMPAT_CLIENT_INSTALL_PATH")?;
            resolve_from_libraries(Path::new(steam_root), &appid)
        });

    let Some((steamapps_path, manifest)) = manifest else {
        return AppMeta {
            compatdata_path: Some(compatdata_path),
            ..AppMeta::default()
        };
    };

    let install_path = manifest
        .installdir
        .map(|dir| steamapps_path.join("common").join(dir));

    AppMeta {
        name: manifest.name,
        install_path,
        compatdata_path: Some(compatdata_path),
    }
}

fn steamapps_path_from_compat<'a>(compat_data: &'a Path, appid: &str) -> Option<&'a Path> {
    let compat_dir = compat_data.parent()?;
    let steamapps = compat_dir.parent()?;
    (compat_data.file_name()? == appid
        && compat_dir.file_name()? == "compatdata"
        && steamapps.file_name()? == "steamapps")
        .then_some(steamapps)
}

fn read_manifest(steamapps_path: &Path, appid: &str) -> Option<(PathBuf, ManifestInfo)> {
    let content =
        fs::read_to_string(steamapps_path.join(format!("appmanifest_{appid}.acf"))).ok()?;
    Some((steamapps_path.to_owned(), parse_manifest_info(&content)))
}

fn resolve_from_libraries(steam_root: &Path, appid: &str) -> Option<(PathBuf, ManifestInfo)> {
    let root_steamapps = steam_root.join("steamapps");
    if let Some(manifest) = read_manifest(&root_steamapps, appid) {
        return Some(manifest);
    }

    let content = fs::read_to_string(root_steamapps.join("libraryfolders.vdf")).ok()?;
    parse_library_paths(&content)
        .map(|library| Path::new(library).join("steamapps"))
        .find_map(|steamapps| read_manifest(&steamapps, appid))
}

fn parse_library_paths(content: &str) -> impl Iterator<Item = &str> {
    content.lines().filter_map(|line| {
        let (key, value) = parse_acf_line(line)?;
        (key == "path").then_some(value)
    })
}

fn parse_manifest_info(content: &str) -> ManifestInfo {
    let mut info = ManifestInfo::default();

    for line in content.lines() {
        let Some((key, value)) = parse_acf_line(line) else {
            continue;
        };

        match key {
            "name" => info.name = Some(value.to_string()),
            "installdir" => info.installdir = Some(value.to_string()),
            _ => {}
        }

        if info.name.is_some() && info.installdir.is_some() {
            break;
        }
    }

    info
}

fn parse_acf_line(line: &str) -> Option<(&str, &str)> {
    let mut tokens = line
        .trim()
        .split('"')
        .map(str::trim)
        .filter(|token| !token.is_empty());

    let key = tokens.next()?;
    let value = tokens.next()?;
    Some((key, value))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{parse_library_paths, parse_manifest_info, steamapps_path_from_compat};

    #[test]
    fn parses_manifest_fields() {
        let manifest = r#"
            "AppState"
            {
                "appid"      "1217060"
                "name"       "Gunfire Reborn"
                "installdir" "Gunfire Reborn"
            }
        "#;

        let info = parse_manifest_info(manifest);
        assert_eq!(info.name.as_deref(), Some("Gunfire Reborn"));
        assert_eq!(info.installdir.as_deref(), Some("Gunfire Reborn"));
    }

    #[test]
    fn parses_library_paths() {
        let folders = r#"
            "libraryfolders"
            {
                "0"
                {
                    "path" "/home/user/.local/share/Steam"
                }
                "1"
                {
                    "path" "/mnt/games/SteamLibrary"
                }
            }
        "#;

        assert_eq!(
            parse_library_paths(folders).collect::<Vec<_>>(),
            ["/home/user/.local/share/Steam", "/mnt/games/SteamLibrary"]
        );
    }

    #[test]
    fn accepts_only_expected_compatdata_layout() {
        assert_eq!(
            steamapps_path_from_compat(
                Path::new("/mnt/games/steamapps/compatdata/1217060"),
                "1217060"
            ),
            Some(Path::new("/mnt/games/steamapps"))
        );
        assert_eq!(
            steamapps_path_from_compat(Path::new("/mnt/games/steamapps/compatdata/999"), "1217060"),
            None
        );
    }
}
