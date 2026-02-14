pub fn is_env_assignment(s: &str) -> bool {
    // Detect leading VAR=VALUE shell-style assignment (VAR must match [A-Za-z_][A-Za-z0-9_]*).
    if let Some(eq) = s.find('=') {
        let (name, _) = s.split_at(eq);
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
            _ => return false,
        }
        for c in chars {
            if !(c == '_' || c.is_ascii_alphanumeric()) {
                return false;
            }
        }
        return true;
    }
    false
}

/// Функция для экранирования строки в стиле shell для двойных кавычек.
pub fn shell_escape(s: &str) -> String {
    if s.contains(char::is_whitespace)
        || s.contains('\'')
        || s.contains('\\')
        || s.contains('"')
        || s.contains('$')
    {
        let mut res = String::from("\"");
        for c in s.chars() {
            if matches!(c, '\\' | '"' | '$' | '`') {
                res.push('\\');
            }
            res.push(c);
        }
        res.push('"');
        res
    } else {
        s.to_string()
    }
}

/// Функция для деэкранирования строки в стиле shell из двойных кавычек.
pub fn un_shell_escape(s: &str) -> String {
    // Если строка не в кавычках, возвращаем как есть.
    if !s.starts_with('"') || !s.ends_with('"') {
        return s.to_string();
    }

    let s = &s[1..s.len() - 1];
    let mut res = String::new();
    let mut chars = s.chars().peekable();

    while let Some(&c) = chars.peek() {
        chars.next();
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
    use super::{is_env_assignment, shell_escape, un_shell_escape};

    #[test]
    fn env_assignment_detection() {
        assert!(is_env_assignment("A=1"));
        assert!(is_env_assignment("_A1=1"));
        assert!(!is_env_assignment("1A=1"));
        assert!(!is_env_assignment("A-B=1"));
    }

    #[test]
    fn escape_roundtrip() {
        let original = r#" a b$c\"` "#;
        let escaped = shell_escape(original);
        assert_eq!(un_shell_escape(&escaped), original);
    }
}
