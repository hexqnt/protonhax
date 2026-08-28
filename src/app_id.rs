use std::{fmt, num::ParseIntError, str::FromStr};

#[derive(Debug)]
pub enum ParseAppIdError {
    InvalidCharacters,
    OutOfRange(ParseIntError),
}

impl std::error::Error for ParseAppIdError {}

impl fmt::Display for ParseAppIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCharacters => {
                formatter.write_str("appid must contain only decimal digits")
            }
            Self::OutOfRange(error) => write!(formatter, "appid is out of range: {error}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetSelector {
    Latest,
    AppId(AppId),
    Name(String),
}

impl FromStr for TargetSelector {
    type Err = ParseAppIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("latest") {
            return Ok(Self::Latest);
        }
        if value.bytes().all(|byte| byte.is_ascii_digit()) {
            return value.parse().map(Self::AppId);
        }

        Ok(Self::Name(value.to_owned()))
    }
}

impl fmt::Display for TargetSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Latest => formatter.write_str("latest"),
            Self::AppId(appid) => appid.fmt(formatter),
            Self::Name(name) => name.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AppId(u32);

impl FromStr for AppId {
    type Err = ParseAppIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ParseAppIdError::InvalidCharacters);
        }

        value.parse().map(Self).map_err(ParseAppIdError::OutOfRange)
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppId, TargetSelector};

    #[test]
    fn appid_accepts_only_u32_decimal() {
        assert_eq!("1217060".parse::<AppId>().unwrap().to_string(), "1217060");
        assert!("../1217060".parse::<AppId>().is_err());
        assert!("4294967296".parse::<AppId>().is_err());
    }

    #[test]
    fn selector_never_treats_a_name_as_a_path() {
        assert_eq!(
            "../../tmp".parse::<TargetSelector>().unwrap(),
            TargetSelector::Name("../../tmp".to_owned())
        );
        assert_eq!(
            "LATEST".parse::<TargetSelector>().unwrap(),
            TargetSelector::Latest
        );
    }
}
