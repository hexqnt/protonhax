use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseEnvError {
    MissingEquals,
    InvalidName,
}

impl std::error::Error for ParseEnvError {}

impl fmt::Display for ParseEnvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingEquals => "environment override must have the NAME=VALUE form",
            Self::InvalidName => "invalid environment variable name",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvName(String);

impl EnvName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for EnvName {
    type Err = ParseEnvError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        is_env_name(value)
            .then(|| Self(value.to_owned()))
            .ok_or(ParseEnvError::InvalidName)
    }
}

impl fmt::Display for EnvName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvOverride {
    name: EnvName,
    value: String,
}

impl EnvOverride {
    pub fn name(&self) -> &EnvName {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl FromStr for EnvOverride {
    type Err = ParseEnvError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, value) = value.split_once('=').ok_or(ParseEnvError::MissingEquals)?;
        Ok(Self {
            name: name.parse()?,
            value: value.to_owned(),
        })
    }
}

impl fmt::Display for EnvOverride {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}={}", self.name, self.value)
    }
}

pub fn is_env_assignment(s: &str) -> bool {
    split_env_assignment(s).is_some()
}

pub fn split_env_assignment(s: &str) -> Option<(&str, &str)> {
    let (name, value) = s.split_once('=')?;
    is_env_name(name).then_some((name, value))
}

pub fn is_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'_' | b'A'..=b'Z' | b'a'..=b'z'))
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{EnvName, EnvOverride, is_env_assignment, split_env_assignment};

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
    fn typed_environment_arguments_are_parsed_once() {
        let assignment = "DXVK_LOG_LEVEL=none".parse::<EnvOverride>().unwrap();
        assert_eq!(assignment.name().as_str(), "DXVK_LOG_LEVEL");
        assert_eq!(assignment.value(), "none");
        assert!("BAD-NAME".parse::<EnvName>().is_err());
        assert!("MISSING_VALUE".parse::<EnvOverride>().is_err());
    }
}
