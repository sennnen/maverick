//! The one error type the core returns, everywhere.
//!
//! Every failure carries a stable numeric code so that logs, the error journal and user bug
//! reports stay meaningful across releases. Codes are append-only: once a number has shipped it is
//! never reused or renumbered, and every code is documented in docs/errors.md. Each category owns
//! a thousand-wide range, and the category is derived from the code so the two can never disagree.

use serde::{Deserialize, Serialize};
use std::fmt;

pub type Result<T> = std::result::Result<T, MavError>;

/// Which layer of the pipeline the error belongs to. Derived from the code's range.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Transport,
    Frame,
    Decode,
    Timeline,
    Storage,
    Feature,
    Analytic,
    Ml,
    Ffi,
    Internal,
}

impl Category {
    const fn for_code(code: u16) -> Self {
        match code {
            1000..=1999 => Category::Transport,
            2000..=2999 => Category::Frame,
            3000..=3999 => Category::Decode,
            4000..=4999 => Category::Timeline,
            5000..=5999 => Category::Storage,
            6000..=6999 => Category::Feature,
            7000..=7999 => Category::Analytic,
            8000..=8999 => Category::Ml,
            9000..=9999 => Category::Ffi,
            _ => Category::Internal,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Data was degraded or discarded but the pipeline continues.
    Warning,
    /// The current unit of work failed; the session continues.
    Error,
    /// The invariants of the store or the core are in doubt; stop and surface it.
    Fatal,
}

/// Stable error codes. Append new ones at the end of their category range; never renumber.
/// The catalogue with meanings lives in docs/errors.md, and every assigned code must also be
/// listed in `ALL` below — tests hold the constants, the list, and the document in step.
pub mod codes {
    pub const FRAME_HEADER_CRC_MISMATCH: u16 = 2001;
    pub const FRAME_PAYLOAD_CRC_MISMATCH: u16 = 2002;
    pub const FRAME_TRUNCATED: u16 = 2003;
    pub const FRAME_OVERSIZED: u16 = 2004;
    pub const FRAME_GARBAGE_SKIPPED: u16 = 2005;
    pub const FRAME_READER_OUT_OF_BOUNDS: u16 = 2006;

    pub const DECODE_UNKNOWN_PACKET_TYPE: u16 = 3001;
    pub const DECODE_LAYOUT_INVALID: u16 = 3002;
    pub const DECODE_FIELD_UNREADABLE: u16 = 3003;
    pub const DECODE_NO_MANIFEST_FOR_MODEL: u16 = 3004;

    pub const TIMELINE_IMPLAUSIBLE_TIMESTAMP: u16 = 4001;

    pub const INTERNAL_INVARIANT: u16 = 10_000;

    pub const ALL: &[(u16, &str)] = &[
        (FRAME_HEADER_CRC_MISMATCH, "FRAME_HEADER_CRC_MISMATCH"),
        (FRAME_PAYLOAD_CRC_MISMATCH, "FRAME_PAYLOAD_CRC_MISMATCH"),
        (FRAME_TRUNCATED, "FRAME_TRUNCATED"),
        (FRAME_OVERSIZED, "FRAME_OVERSIZED"),
        (FRAME_GARBAGE_SKIPPED, "FRAME_GARBAGE_SKIPPED"),
        (FRAME_READER_OUT_OF_BOUNDS, "FRAME_READER_OUT_OF_BOUNDS"),
        (DECODE_UNKNOWN_PACKET_TYPE, "DECODE_UNKNOWN_PACKET_TYPE"),
        (DECODE_LAYOUT_INVALID, "DECODE_LAYOUT_INVALID"),
        (DECODE_FIELD_UNREADABLE, "DECODE_FIELD_UNREADABLE"),
        (DECODE_NO_MANIFEST_FOR_MODEL, "DECODE_NO_MANIFEST_FOR_MODEL"),
        (
            TIMELINE_IMPLAUSIBLE_TIMESTAMP,
            "TIMELINE_IMPLAUSIBLE_TIMESTAMP",
        ),
        (INTERNAL_INVARIANT, "INTERNAL_INVARIANT"),
    ];
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MavError {
    pub code: u16,
    pub category: Category,
    pub severity: Severity,
    pub message: String,
    /// Innermost context first. Grown with `context()` as the error travels up the stack.
    pub context: Vec<String>,
}

impl MavError {
    pub fn new(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            category: Category::for_code(code),
            severity: Severity::Error,
            message: message.into(),
            context: Vec::new(),
        }
    }

    pub fn warning(code: u16, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::new(code, message)
        }
    }

    pub fn fatal(code: u16, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Fatal,
            ..Self::new(code, message)
        }
    }

    pub fn context(mut self, note: impl Into<String>) -> Self {
        self.context.push(note.into());
        self
    }
}

impl fmt::Display for MavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MAV-{} [{:?}/{:?}] {}",
            self.code, self.category, self.severity, self.message
        )?;
        for note in &self.context {
            write!(f, "; {note}")?;
        }
        Ok(())
    }
}

impl std::error::Error for MavError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_is_derived_from_code_range() {
        assert_eq!(MavError::new(1000, "x").category, Category::Transport);
        assert_eq!(
            MavError::new(codes::FRAME_TRUNCATED, "x").category,
            Category::Frame
        );
        assert_eq!(MavError::new(3500, "x").category, Category::Decode);
        assert_eq!(MavError::new(9999, "x").category, Category::Ffi);
        assert_eq!(
            MavError::new(codes::INTERNAL_INVARIANT, "x").category,
            Category::Internal
        );
        assert_eq!(MavError::new(0, "x").category, Category::Internal);
    }

    #[test]
    fn context_accumulates_in_order() {
        let err = MavError::new(codes::FRAME_PAYLOAD_CRC_MISMATCH, "crc32 mismatch")
            .context("frame 12")
            .context("session 3");
        assert_eq!(
            err.context,
            vec!["frame 12".to_owned(), "session 3".to_owned()]
        );
        let shown = err.to_string();
        assert!(
            shown.starts_with("MAV-2002 [Frame/Error] crc32 mismatch"),
            "{shown}"
        );
        assert!(shown.ends_with("frame 12; session 3"), "{shown}");
    }

    #[test]
    fn severity_constructors() {
        assert_eq!(
            MavError::warning(codes::FRAME_GARBAGE_SKIPPED, "x").severity,
            Severity::Warning
        );
        assert_eq!(
            MavError::fatal(codes::INTERNAL_INVARIANT, "x").severity,
            Severity::Fatal
        );
        assert_eq!(
            MavError::new(codes::FRAME_TRUNCATED, "x").severity,
            Severity::Error
        );
    }

    #[test]
    fn code_registry_is_unique_and_in_range() {
        let mut seen = std::collections::HashSet::new();
        for &(code, name) in codes::ALL {
            assert!(seen.insert(code), "code {code} listed twice in codes::ALL");
            assert!(!name.is_empty());
            if code < 10_000 {
                assert_ne!(
                    Category::for_code(code),
                    Category::Internal,
                    "code {code} ({name}) falls outside every category range",
                );
            }
        }
        assert!(seen.contains(&codes::FRAME_HEADER_CRC_MISMATCH));
        assert!(seen.contains(&codes::INTERNAL_INVARIANT));
    }

    #[test]
    fn serialises_for_the_error_journal() {
        let err = MavError::new(codes::FRAME_HEADER_CRC_MISMATCH, "bad header");
        let json = serde_json::to_string(&err).unwrap();
        let back: MavError = serde_json::from_str(&json).unwrap();
        assert_eq!(back, err);
    }
}
