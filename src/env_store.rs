use std::{env, fs, io, path::Path};

use crate::shell::un_shell_escape;

pub fn load_env<P: AsRef<Path>>(app_dir: P) -> Result<(), io::Error> {
    let env_content = fs::read_to_string(app_dir.as_ref().join("env"))?;
    apply_env_content(&env_content);
    Ok(())
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

pub fn set_env_var(name: &str, value: &str) {
    // SAFETY: the CLI is single-threaded and mutates the process environment
    // only during command setup, before waiting on child processes.
    unsafe {
        env::set_var(name, value);
    }
}

fn apply_env_content(env_content: &str) {
    for line in env_content.lines() {
        if let Some((name, value_str)) = parse_export_line(line) {
            set_env_var(name, &un_shell_escape(value_str));
        }
    }
}

fn parse_export_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("declare -x ")?;
    let eq_idx = rest.find('=')?;
    let name = rest[..eq_idx].trim();
    let value_str = rest[eq_idx + 1..].trim();
    Some((name, value_str))
}
