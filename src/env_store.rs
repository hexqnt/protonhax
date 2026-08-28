use std::{fs, io, path::Path, process::Command};

use crate::shell::{is_env_name, un_shell_escape};

pub const ENV_FILE: &str = "env";

pub struct StoredEnv(Vec<(String, String)>);

impl StoredEnv {
    /// Загружает и разбирает сохранённое окружение один раз на границе системы.
    pub fn load(app_dir: &Path) -> io::Result<Self> {
        let env_content = fs::read_to_string(app_dir.join(ENV_FILE))?;
        Ok(Self::parse(&env_content))
    }

    fn parse(env_content: &str) -> Self {
        let vars = env_content
            .lines()
            .filter_map(parse_export_line)
            .map(|(name, value)| (name.to_owned(), un_shell_escape(value)))
            .collect();

        Self(vars)
    }

    /// Добавляет окружение только в дочерний процесс, не изменяя процесс protonhax.
    pub fn apply_to(&self, command: &mut Command) {
        command.envs(self.0.iter().map(|(name, value)| (name, value)));
    }
}

pub fn get_env_var(env_content: &str, key: &str) -> Option<String> {
    for line in env_content.lines() {
        if let Some((name, value_str)) = parse_export_line(line)
            && name == key
        {
            return Some(un_shell_escape(value_str));
        }
    }
    None
}

fn parse_export_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("declare -x ")?;
    let eq_idx = rest.find('=')?;
    let name = rest[..eq_idx].trim();
    let value_str = rest[eq_idx + 1..].trim();
    is_env_name(name).then_some((name, value_str))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::{StoredEnv, get_env_var, parse_export_line};

    #[test]
    fn parses_export_line_with_valid_name() {
        assert_eq!(
            parse_export_line(r#"declare -x STEAM_COMPAT_DATA_PATH="/tmp/compat data""#),
            Some(("STEAM_COMPAT_DATA_PATH", r#""/tmp/compat data""#))
        );
    }

    #[test]
    fn rejects_export_line_with_invalid_name() {
        assert_eq!(parse_export_line("declare -x 1BAD=value"), None);
    }

    #[test]
    fn reads_shell_escaped_env_value() {
        let env_content = r#"declare -x KEY="a b\$c""#;
        assert_eq!(get_env_var(env_content, "KEY").as_deref(), Some("a b$c"));
    }

    #[test]
    fn applies_only_parsed_variables_to_command() {
        let env = StoredEnv::parse(
            "declare -x VALID=one\ndeclare -x QUOTED=\"two words\"\ndeclare -x 1BAD=nope",
        );
        let mut command = Command::new("true");
        env.apply_to(&mut command);

        let vars: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(vars.len(), 2);
        assert_eq!(
            vars.get(std::ffi::OsStr::new("VALID"))
                .and_then(|value| value.and_then(|value| value.to_str())),
            Some("one")
        );
        assert_eq!(
            vars.get(std::ffi::OsStr::new("QUOTED"))
                .and_then(|value| value.and_then(|value| value.to_str())),
            Some("two words")
        );
    }
}
