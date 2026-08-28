use std::{fmt, fs, io, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionId {
    pid: u32,
    start_ticks: u64,
}

impl SessionId {
    pub fn for_pid(pid: u32) -> io::Result<Self> {
        Ok(Self {
            pid,
            start_ticks: process_start_ticks(pid)?,
        })
    }

    pub fn is_active(self) -> bool {
        process_start_ticks(self.pid).is_ok_and(|start_ticks| start_ticks == self.start_ticks)
    }

    pub fn pid(self) -> u32 {
        self.pid
    }

    pub(super) fn recency(self) -> (u64, u32) {
        (self.start_ticks, self.pid)
    }
}

impl FromStr for SessionId {
    type Err = io::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (pid, start_ticks) = value.split_once('-').ok_or_else(invalid_session_id)?;
        if pid.is_empty()
            || start_ticks.is_empty()
            || !pid.bytes().all(|byte| byte.is_ascii_digit())
            || !start_ticks.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid_session_id());
        }

        Ok(Self {
            pid: pid.parse().map_err(|_| invalid_session_id())?,
            start_ticks: start_ticks.parse().map_err(|_| invalid_session_id())?,
        })
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}-{}", self.pid, self.start_ticks)
    }
}

fn process_start_ticks(pid: u32) -> io::Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close_paren = stat
        .rfind(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid process stat format"))?;
    stat[close_paren + 1..]
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process start time is missing"))?
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn invalid_session_id() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid session id")
}

#[cfg(test)]
mod tests {
    use std::{process, str::FromStr};

    use super::SessionId;

    #[test]
    fn current_process_session_is_active() {
        let session = SessionId::for_pid(process::id()).unwrap();
        assert!(session.is_active());
        assert_eq!(SessionId::from_str(&session.to_string()).unwrap(), session);
    }

    #[test]
    fn session_id_rejects_path_components() {
        assert!(SessionId::from_str("../1-2").is_err());
        assert!(SessionId::from_str("1-2/3").is_err());
    }
}
