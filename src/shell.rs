pub fn is_env_assignment(s: &str) -> bool {
    s.split_once('=').is_some_and(|(name, _)| is_env_name(name))
}

pub fn split_env_assignment(s: &str) -> Option<(&str, &str)> {
    let (name, value) = s.split_once('=')?;
    is_env_name(name).then_some((name, value))
}

pub fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Функция для экранирования строки в стиле shell для двойных кавычек.
pub fn shell_escape(s: &str) -> String {
    if !needs_shell_quotes(s) {
        return s.to_string();
    }

    let extra_escapes = s
        .chars()
        .filter(|c| matches!(c, '\\' | '"' | '$' | '`'))
        .count();
    let mut res = String::with_capacity(s.len() + extra_escapes + 2);
    res.push('"');
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            res.push('\\');
        }
        res.push(c);
    }
    res.push('"');
    res
}

fn needs_shell_quotes(s: &str) -> bool {
    s.contains(char::is_whitespace)
        || s.contains('\'')
        || s.contains('\\')
        || s.contains('"')
        || s.contains('$')
        || s.contains('`')
}

/// Функция для деэкранирования строки в стиле shell из двойных кавычек.
pub fn un_shell_escape(s: &str) -> String {
    // Если строка не в кавычках, возвращаем как есть.
    if !s.starts_with('"') || !s.ends_with('"') {
        return s.to_string();
    }

    let s = &s[1..s.len() - 1];
    let mut res = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    '\\' => res.push('\\'),
                    '"' => res.push('"'),
                    '$' => res.push('$'),
                    '`' => res.push('`'),
                    _ => {
                        res.push('\\');
                        res.push(next);
                    }
                }
            } else {
                res.push('\\');
            }
        } else {
            res.push(c);
        }
    }

    res
}

#[cfg(test)]
mod tests {
    use super::{is_env_assignment, shell_escape, split_env_assignment, un_shell_escape};

    #[test]
    fn env_assignment_detection() {
        assert!(is_env_assignment("A=1"));
        assert!(is_env_assignment("_A1=1"));
        assert!(!is_env_assignment("1A=1"));
        assert!(!is_env_assignment("A-B=1"));
    }

    #[test]
    fn env_assignment_split() {
        assert_eq!(split_env_assignment("A=1=2"), Some(("A", "1=2")));
        assert_eq!(split_env_assignment("A-B=1"), None);
    }

    #[test]
    fn escape_roundtrip() {
        let original = r#" a b$c\"` "#;
        let escaped = shell_escape(original);
        assert_eq!(un_shell_escape(&escaped), original);
    }

    #[test]
    fn backtick_is_escaped() {
        let escaped = shell_escape("`uname`");
        assert_eq!(escaped, "\"\\`uname\\`\"");
        assert_eq!(un_shell_escape(&escaped), "`uname`");
    }
}
