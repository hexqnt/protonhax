use std::{fs, path::Path};

use crate::env_store::get_env_var;

#[derive(Default)]
pub struct AppMeta {
    pub name: Option<String>,
    pub install_path: Option<String>,
}

#[derive(Default)]
struct ManifestInfo {
    name: Option<String>,
    installdir: Option<String>,
}

pub fn resolve_app_meta(app_dir: &Path, appid: &str) -> AppMeta {
    let Ok(env_content) = fs::read_to_string(app_dir.join("env")) else {
        return AppMeta::default();
    };

    let Some(compat_data) = get_env_var(&env_content, "STEAM_COMPAT_DATA_PATH") else {
        return AppMeta::default();
    };

    let Some(steamapps_path) = steamapps_path_from_compat(&compat_data) else {
        return AppMeta::default();
    };

    let manifest_path = steamapps_path.join(format!("appmanifest_{appid}.acf"));
    let Ok(manifest_content) = fs::read_to_string(manifest_path) else {
        return AppMeta::default();
    };

    let manifest = parse_manifest_info(&manifest_content);
    let install_path = manifest.installdir.map(|dir| {
        steamapps_path
            .join("common")
            .join(dir)
            .to_string_lossy()
            .into_owned()
    });

    AppMeta {
        name: manifest.name,
        install_path,
    }
}

fn steamapps_path_from_compat(compat_data: &str) -> Option<&Path> {
    Path::new(compat_data).parent()?.parent()
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
    use super::parse_manifest_info;

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
}
