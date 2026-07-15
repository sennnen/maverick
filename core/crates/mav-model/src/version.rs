//! A small semantic version, used to stamp algorithms, fixtures, manifests and the storage schema.
//! Fixtures record the versions they were produced with, and the engine's recompute cache is keyed
//! on them, so equality and ordering must be exact and boring.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseVersionError(String);

impl fmt::Display for ParseVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a version string: {:?}", self.0)
    }
}

impl std::error::Error for ParseVersionError {}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('.');
        let mut next = || -> Result<u16, ParseVersionError> {
            parts
                .next()
                .ok_or_else(|| ParseVersionError(s.to_owned()))?
                .parse()
                .map_err(|_| ParseVersionError(s.to_owned()))
        };
        let version = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        if parts.next().is_some() {
            return Err(ParseVersionError(s.to_owned()));
        }
        Ok(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_field_by_field() {
        assert!(Version::new(2, 0, 0) > Version::new(1, 9, 9));
        assert!(Version::new(1, 2, 0) > Version::new(1, 1, 9));
        assert!(Version::new(1, 1, 2) > Version::new(1, 1, 1));
    }

    #[test]
    fn parses_and_displays() {
        let v: Version = "3.14.1".parse().unwrap();
        assert_eq!(v, Version::new(3, 14, 1));
        assert_eq!(v.to_string(), "3.14.1");
    }

    #[test]
    fn rejects_malformed_strings() {
        for bad in ["", "1", "1.2", "1.2.3.4", "1.2.x", "v1.2.3", "1..3"] {
            assert!(bad.parse::<Version>().is_err(), "accepted {bad:?}");
        }
    }
}
