//! The Gateway version check: does the Gateway Workshop attached to meet
//! the lowest release the pin allows?
//!
//! After the split, Workshop and the Gateway no longer come from one
//! commit, so a running Gateway may be older than the pin. Nothing else
//! proves the API surface matches: the pin's `min_gateway` is the lowest
//! release with every operation Workshop calls, so a Gateway below it is
//! missing calls that exist only in later releases.
//!
//! Both versions are logged, and a Gateway below the minimum is announced
//! on the status bar through the same push facade every other startup
//! phase uses. The versions are compared as dotted numbers, never as
//! strings: `0.10.0` is newer than `0.9.0`, and a string compare says the
//! opposite.
//!
//! A pin that records no minimum turns the check off, which is a problem
//! with the build rather than with the Gateway. A release build says so on
//! the status bar: nothing then guards the Gateway it ships. A debug build
//! says so in the log only, because it reaches that state on every launch
//! while the pin is still a placeholder, and a status line for it would be
//! noise a developer learns to ignore.

use std::cmp::Ordering;
use std::fmt;

use workshop_protocol::Activity;
use workshop_registry::Push;

/// The `min_gateway` value the pin recorded when Workshop was built, or
/// an empty string when the pin was still a placeholder.
pub const MIN_GATEWAY: &str = env!("RUNVIL_MIN_GATEWAY");

/// A dotted version: a major, a minor, and a patch number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    /// Parses `major.minor.patch`, tolerating a leading `v` and ignoring a
    /// prerelease or build suffix. Returns `None` for anything else, so a
    /// version nobody can read is never silently treated as satisfying a
    /// minimum.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim().trim_start_matches('v');
        let core = trimmed
            .split(['-', '+'])
            .next()
            .filter(|core| !core.is_empty())?;
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Whether this version is at least `floor`.
    #[must_use]
    pub fn is_at_least(self, floor: Self) -> bool {
        self >= floor
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCheck {
    /// The pin recorded no minimum, because it is still a placeholder.
    NoMinimumRecorded,
    /// The running Gateway meets the pin's minimum.
    Satisfied {
        /// The Gateway's version, as the discovery file reported it.
        running: Version,
        /// The pin's `min_gateway`.
        minimum: Version,
    },
    /// The running Gateway is older than the pin's minimum.
    TooOld {
        /// The Gateway's version, as the discovery file reported it.
        running: Version,
        /// The pin's `min_gateway`.
        minimum: Version,
    },
    /// Either version could not be read as numbers. Reported rather than
    /// assumed either way.
    Unreadable {
        /// The Gateway's version, verbatim.
        running: String,
        /// The pin's `min_gateway`, verbatim.
        minimum: String,
    },
}

impl VersionCheck {
    /// Compares a running Gateway's version against a minimum, both as
    /// text. An empty `minimum` means the pin recorded none.
    #[must_use]
    pub fn compare(running: &str, minimum: &str) -> Self {
        if minimum.trim().is_empty() {
            return Self::NoMinimumRecorded;
        }
        match (Version::parse(running), Version::parse(minimum)) {
            (Some(running), Some(minimum)) if running.is_at_least(minimum) => {
                Self::Satisfied { running, minimum }
            }
            (Some(running), Some(minimum)) => Self::TooOld { running, minimum },
            _ => Self::Unreadable {
                running: running.to_owned(),
                minimum: minimum.to_owned(),
            },
        }
    }

    /// The check against the pin's recorded minimum.
    #[must_use]
    pub fn against_pin(running: &str) -> Self {
        Self::compare(running, MIN_GATEWAY)
    }

    /// Whether this check should be shown to the user.
    #[must_use]
    pub fn is_problem(&self) -> bool {
        matches!(self, Self::TooOld { .. } | Self::Unreadable { .. })
    }
}

/// Logs both versions and, when the running Gateway is below the pin's
/// minimum or its version cannot be read, warns on the status bar. A
/// satisfied check logs and says nothing: every healthy boot looks like
/// that, and a status line for it would be noise.
pub fn report(running: &str, push: &Push) {
    announce(&VersionCheck::against_pin(running), push);
}

/// Logs `check` and warns on the status bar when it is a problem. Split
/// from [`report`] so a test can drive any outcome, including one the
/// committed pin cannot currently produce.
///
/// An unfilled pin reaches the status bar in a release build only; the
/// build split lives in `announce_for`.
pub fn announce(check: &VersionCheck, push: &Push) {
    announce_for(check, push, Build::current());
}

/// Which kind of build is asking. An unfilled pin is a defect in a
/// shipped build and a normal state in a developer's checkout, so the
/// status bar is told about the first and not the second.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Build {
    /// A debug build: the developer's own `cargo build` or `cargo test`.
    Debug,
    /// A release build, which is what an installer ships.
    Release,
}

impl Build {
    /// The build this code was compiled into.
    fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Debug
        } else {
            Self::Release
        }
    }
}

/// Logs `check` and warns on the status bar when it is a problem. `build`
/// selects whether an unfilled pin also reaches the status bar.
fn announce_for(check: &VersionCheck, push: &Push, build: Build) {
    match check {
        VersionCheck::NoMinimumRecorded => {
            tracing::warn!("the Gateway pin records no minimum version; the version check is off");
            // A release build is what an installer ships. An unfilled pin
            // there leaves the app with no version guard at all, so the
            // user sees it. A debug build is a developer's own checkout,
            // where the pin stays a placeholder until a Gateway release
            // exists, so it stays in the log.
            if build == Build::Release {
                push.push_status_update(
                    "Gateway version unchecked",
                    "this Workshop was built from a pin with no minimum version, so nothing \
                     checks the Gateway it attaches to",
                    Activity::General,
                );
            }
        }
        VersionCheck::Satisfied { running, minimum } => {
            tracing::info!(
                gateway_version = %running,
                minimum_gateway = %minimum,
                "the Gateway meets the pin's minimum version"
            );
        }
        VersionCheck::TooOld { running, minimum } => {
            tracing::warn!(
                gateway_version = %running,
                minimum_gateway = %minimum,
                "the Gateway is older than the pin's minimum version"
            );
            push.push_status_update(
                "Gateway too old",
                format!(
                    "the running Gateway is {running} but this Workshop needs at least {minimum}; \
                     install a newer Gateway"
                ),
                Activity::General,
            );
        }
        VersionCheck::Unreadable { running, minimum } => {
            tracing::warn!(
                gateway_version = running,
                minimum_gateway = minimum,
                "a Gateway version could not be compared with the pin's minimum"
            );
            push.push_status_update(
                "Gateway version unknown",
                format!(
                    "could not compare the running Gateway's version ({running}) with the \
                     required {minimum}"
                ),
                Activity::General,
            );
        }
    }
}

#[cfg(test)]
#[path = "version-tests.rs"]
mod tests;
