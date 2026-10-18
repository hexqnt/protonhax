use std::{ffi::OsStr, fmt};

use serde::{Serialize, Serializer, ser::SerializeSeq};

use crate::{
    app_id::TargetSelector,
    env_store::is_sensitive_name,
    launch::RunCwd,
    shell::{EnvName, EnvOverride},
};

use super::{Profile, ProfileKind};

#[derive(Serialize)]
pub(super) struct ProfileView<'a> {
    target: DisplayValue<&'a TargetSelector>,
    kind: DisplayValue<ProfileKind>,
    command: &'a [String],
    detach: bool,
    cwd: Option<DisplayValue<&'a RunCwd>>,
    environment: EnvironmentView<'a>,
    #[serde(serialize_with = "serialize_display_slice")]
    unset_environment: &'a [EnvName],
}

impl<'a> ProfileView<'a> {
    pub(super) fn new(profile: &'a Profile, redact: bool) -> Self {
        Self {
            target: DisplayValue(&profile.target),
            kind: DisplayValue(profile.kind),
            command: &profile.command,
            detach: profile.run_options.detach,
            cwd: profile.run_options.cwd.as_ref().map(DisplayValue),
            environment: EnvironmentView {
                assignments: &profile.run_options.environment.overrides,
                redact,
            },
            unset_environment: &profile.run_options.environment.removals,
        }
    }
}

struct DisplayValue<T>(T);

impl<T: fmt::Display> Serialize for DisplayValue<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

fn serialize_display_slice<S: Serializer, T: fmt::Display>(
    values: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(values.iter().map(DisplayValue))
}

struct EnvironmentView<'a> {
    assignments: &'a [EnvOverride],
    redact: bool,
}

impl Serialize for EnvironmentView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.assignments.len()))?;
        for assignment in self.assignments {
            if self.redact && is_sensitive_name(OsStr::new(assignment.name().as_str())) {
                sequence.serialize_element(&DisplayValue(format_args!(
                    "{}=<redacted>",
                    assignment.name()
                )))?;
            } else {
                sequence.serialize_element(&DisplayValue(assignment))?;
            }
        }
        sequence.end()
    }
}
