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
    Connector,
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
            11_000..=11_999 => Category::Connector,
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
    pub const TRANSPORT_COMMAND_TIMEOUT: u16 = 1001;
    pub const TRANSPORT_UNEXPECTED_RESPONSE: u16 = 1002;
    pub const TRANSPORT_UNEXPECTED_BYTES: u16 = 1003;
    pub const TRANSPORT_HISTORICAL_PROTOCOL: u16 = 1004;
    pub const TRANSPORT_COMMAND_REJECTED: u16 = 1005;
    pub const TRANSPORT_NATIVE_FAILURE: u16 = 1006;

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
    pub const DECODE_UNKNOWN_RECORD_VERSION: u16 = 3005;
    pub const DECODE_CODEC_UNAVAILABLE: u16 = 3006;

    pub const TIMELINE_IMPLAUSIBLE_TIMESTAMP: u16 = 4001;

    pub const STORAGE_OPEN: u16 = 5001;
    pub const STORAGE_MIGRATION: u16 = 5002;
    pub const STORAGE_NEWER_SCHEMA: u16 = 5003;
    pub const STORAGE_QUERY: u16 = 5004;
    pub const STORAGE_SERIALIZE: u16 = 5005;

    pub const FFI_RUNTIME_STATE: u16 = 9001;
    pub const FFI_ACTION_QUEUE_FULL: u16 = 9002;
    pub const FFI_CONNECTOR_NOT_FOUND: u16 = 9003;
    pub const FFI_CONNECTOR_DOWNGRADE: u16 = 9004;

    pub const CONNECTOR_ARTIFACT_OVERSIZED: u16 = 11_001;
    pub const CONNECTOR_ARTIFACT_MALFORMED_WASM: u16 = 11_002;
    pub const CONNECTOR_ARTIFACT_SECTION_MISSING: u16 = 11_003;
    pub const CONNECTOR_ARTIFACT_SECTION_DUPLICATE: u16 = 11_004;
    pub const CONNECTOR_ARTIFACT_SECTION_ORDER: u16 = 11_005;
    pub const CONNECTOR_ARTIFACT_UNKNOWN_CRITICAL_SECTION: u16 = 11_006;
    pub const CONNECTOR_ARTIFACT_SECTION_OVERSIZED: u16 = 11_007;
    pub const CONNECTOR_ARTIFACT_NONCANONICAL_CBOR: u16 = 11_008;
    pub const CONNECTOR_ARTIFACT_DIGEST_MISMATCH: u16 = 11_009;
    pub const CONNECTOR_TRUST_UNKNOWN_PUBLISHER: u16 = 11_010;
    pub const CONNECTOR_TRUST_KEY_NOT_YET_VALID: u16 = 11_011;
    pub const CONNECTOR_TRUST_KEY_EXPIRED: u16 = 11_012;
    pub const CONNECTOR_TRUST_KEY_REVOKED: u16 = 11_013;
    pub const CONNECTOR_TRUST_KEY_ROTATED: u16 = 11_014;
    pub const CONNECTOR_TRUST_SCOPE_REJECTED: u16 = 11_015;
    pub const CONNECTOR_TRUST_SIGNATURE_INVALID: u16 = 11_016;
    pub const CONNECTOR_TRUST_POLICY_INVALID: u16 = 11_017;
    pub const CONNECTOR_TRUST_REVOCATION_STALE: u16 = 11_018;
    pub const CONNECTOR_RUNTIME_LIMIT_PROFILE: u16 = 11_019;
    pub const CONNECTOR_RUNTIME_IMPORT_FORBIDDEN: u16 = 11_020;
    pub const CONNECTOR_RUNTIME_FEATURE_FORBIDDEN: u16 = 11_021;
    pub const CONNECTOR_RUNTIME_EXPORT_INVALID: u16 = 11_022;
    pub const CONNECTOR_RUNTIME_MODULE_LIMIT: u16 = 11_023;
    pub const CONNECTOR_RUNTIME_INSTANTIATION: u16 = 11_024;
    pub const CONNECTOR_RUNTIME_FUEL_EXHAUSTED: u16 = 11_025;
    pub const CONNECTOR_RUNTIME_STACK_LIMIT: u16 = 11_026;
    pub const CONNECTOR_RUNTIME_RESOURCE_LIMIT: u16 = 11_027;
    pub const CONNECTOR_RUNTIME_TRAP: u16 = 11_028;
    pub const CONNECTOR_RUNTIME_MEMORY_ACCESS: u16 = 11_029;
    pub const CONNECTOR_RUNTIME_INPUT_OVERSIZED: u16 = 11_030;
    pub const CONNECTOR_RUNTIME_OUTPUT_OVERSIZED: u16 = 11_031;
    pub const CONNECTOR_RUNTIME_OUTPUT_INVALID: u16 = 11_032;
    pub const CONNECTOR_RUNTIME_STATE_OVERSIZED: u16 = 11_033;
    pub const CONNECTOR_RUNTIME_INSTANCE_UNUSABLE: u16 = 11_034;
    pub const CONNECTOR_RUNTIME_INPUT_INVALID: u16 = 11_035;
    pub const CONNECTOR_RUNTIME_FIXTURE_INVALID: u16 = 11_036;
    pub const CONNECTOR_RUNTIME_FIXTURE_MISMATCH: u16 = 11_037;
    pub const CONNECTOR_HOST_STATE: u16 = 11_038;
    pub const CONNECTOR_HOST_ACTION_INVALID: u16 = 11_039;
    pub const CONNECTOR_HOST_ACTION_UNDECLARED: u16 = 11_040;
    pub const CONNECTOR_HOST_QUEUE_FULL: u16 = 11_041;
    pub const CONNECTOR_HOST_RESULT_MISMATCH: u16 = 11_042;
    pub const CONNECTOR_HOST_SAMPLE_INVALID: u16 = 11_043;
    pub const CONNECTOR_HOST_LATE_RESULT: u16 = 11_044;
    pub const CONNECTOR_HOST_OPERATION_DUPLICATE: u16 = 11_045;
    pub const CONNECTOR_INSTALL_APPROVAL_INVALID: u16 = 11_046;
    pub const CONNECTOR_INSTALL_DOWNGRADE: u16 = 11_047;
    pub const CONNECTOR_INSTALL_NOT_FOUND: u16 = 11_048;
    pub const CONNECTOR_INSTALL_STATE_NAMESPACE: u16 = 11_049;
    pub const CONNECTOR_INSTALL_MIGRATION: u16 = 11_050;
    pub const CONNECTOR_INSTALL_STORAGE: u16 = 11_051;

    pub const INTERNAL_INVARIANT: u16 = 10_000;

    pub const ALL: &[(u16, &str)] = &[
        (TRANSPORT_COMMAND_TIMEOUT, "TRANSPORT_COMMAND_TIMEOUT"),
        (
            TRANSPORT_UNEXPECTED_RESPONSE,
            "TRANSPORT_UNEXPECTED_RESPONSE",
        ),
        (TRANSPORT_UNEXPECTED_BYTES, "TRANSPORT_UNEXPECTED_BYTES"),
        (
            TRANSPORT_HISTORICAL_PROTOCOL,
            "TRANSPORT_HISTORICAL_PROTOCOL",
        ),
        (TRANSPORT_COMMAND_REJECTED, "TRANSPORT_COMMAND_REJECTED"),
        (TRANSPORT_NATIVE_FAILURE, "TRANSPORT_NATIVE_FAILURE"),
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
            DECODE_UNKNOWN_RECORD_VERSION,
            "DECODE_UNKNOWN_RECORD_VERSION",
        ),
        (DECODE_CODEC_UNAVAILABLE, "DECODE_CODEC_UNAVAILABLE"),
        (
            TIMELINE_IMPLAUSIBLE_TIMESTAMP,
            "TIMELINE_IMPLAUSIBLE_TIMESTAMP",
        ),
        (STORAGE_OPEN, "STORAGE_OPEN"),
        (STORAGE_MIGRATION, "STORAGE_MIGRATION"),
        (STORAGE_NEWER_SCHEMA, "STORAGE_NEWER_SCHEMA"),
        (STORAGE_QUERY, "STORAGE_QUERY"),
        (STORAGE_SERIALIZE, "STORAGE_SERIALIZE"),
        (FFI_RUNTIME_STATE, "FFI_RUNTIME_STATE"),
        (FFI_ACTION_QUEUE_FULL, "FFI_ACTION_QUEUE_FULL"),
        (FFI_CONNECTOR_NOT_FOUND, "FFI_CONNECTOR_NOT_FOUND"),
        (FFI_CONNECTOR_DOWNGRADE, "FFI_CONNECTOR_DOWNGRADE"),
        (CONNECTOR_ARTIFACT_OVERSIZED, "CONNECTOR_ARTIFACT_OVERSIZED"),
        (
            CONNECTOR_ARTIFACT_MALFORMED_WASM,
            "CONNECTOR_ARTIFACT_MALFORMED_WASM",
        ),
        (
            CONNECTOR_ARTIFACT_SECTION_MISSING,
            "CONNECTOR_ARTIFACT_SECTION_MISSING",
        ),
        (
            CONNECTOR_ARTIFACT_SECTION_DUPLICATE,
            "CONNECTOR_ARTIFACT_SECTION_DUPLICATE",
        ),
        (
            CONNECTOR_ARTIFACT_SECTION_ORDER,
            "CONNECTOR_ARTIFACT_SECTION_ORDER",
        ),
        (
            CONNECTOR_ARTIFACT_UNKNOWN_CRITICAL_SECTION,
            "CONNECTOR_ARTIFACT_UNKNOWN_CRITICAL_SECTION",
        ),
        (
            CONNECTOR_ARTIFACT_SECTION_OVERSIZED,
            "CONNECTOR_ARTIFACT_SECTION_OVERSIZED",
        ),
        (
            CONNECTOR_ARTIFACT_NONCANONICAL_CBOR,
            "CONNECTOR_ARTIFACT_NONCANONICAL_CBOR",
        ),
        (
            CONNECTOR_ARTIFACT_DIGEST_MISMATCH,
            "CONNECTOR_ARTIFACT_DIGEST_MISMATCH",
        ),
        (
            CONNECTOR_TRUST_UNKNOWN_PUBLISHER,
            "CONNECTOR_TRUST_UNKNOWN_PUBLISHER",
        ),
        (
            CONNECTOR_TRUST_KEY_NOT_YET_VALID,
            "CONNECTOR_TRUST_KEY_NOT_YET_VALID",
        ),
        (CONNECTOR_TRUST_KEY_EXPIRED, "CONNECTOR_TRUST_KEY_EXPIRED"),
        (CONNECTOR_TRUST_KEY_REVOKED, "CONNECTOR_TRUST_KEY_REVOKED"),
        (CONNECTOR_TRUST_KEY_ROTATED, "CONNECTOR_TRUST_KEY_ROTATED"),
        (
            CONNECTOR_TRUST_SCOPE_REJECTED,
            "CONNECTOR_TRUST_SCOPE_REJECTED",
        ),
        (
            CONNECTOR_TRUST_SIGNATURE_INVALID,
            "CONNECTOR_TRUST_SIGNATURE_INVALID",
        ),
        (
            CONNECTOR_TRUST_POLICY_INVALID,
            "CONNECTOR_TRUST_POLICY_INVALID",
        ),
        (
            CONNECTOR_TRUST_REVOCATION_STALE,
            "CONNECTOR_TRUST_REVOCATION_STALE",
        ),
        (
            CONNECTOR_RUNTIME_LIMIT_PROFILE,
            "CONNECTOR_RUNTIME_LIMIT_PROFILE",
        ),
        (
            CONNECTOR_RUNTIME_IMPORT_FORBIDDEN,
            "CONNECTOR_RUNTIME_IMPORT_FORBIDDEN",
        ),
        (
            CONNECTOR_RUNTIME_FEATURE_FORBIDDEN,
            "CONNECTOR_RUNTIME_FEATURE_FORBIDDEN",
        ),
        (
            CONNECTOR_RUNTIME_EXPORT_INVALID,
            "CONNECTOR_RUNTIME_EXPORT_INVALID",
        ),
        (
            CONNECTOR_RUNTIME_MODULE_LIMIT,
            "CONNECTOR_RUNTIME_MODULE_LIMIT",
        ),
        (
            CONNECTOR_RUNTIME_INSTANTIATION,
            "CONNECTOR_RUNTIME_INSTANTIATION",
        ),
        (
            CONNECTOR_RUNTIME_FUEL_EXHAUSTED,
            "CONNECTOR_RUNTIME_FUEL_EXHAUSTED",
        ),
        (
            CONNECTOR_RUNTIME_STACK_LIMIT,
            "CONNECTOR_RUNTIME_STACK_LIMIT",
        ),
        (
            CONNECTOR_RUNTIME_RESOURCE_LIMIT,
            "CONNECTOR_RUNTIME_RESOURCE_LIMIT",
        ),
        (CONNECTOR_RUNTIME_TRAP, "CONNECTOR_RUNTIME_TRAP"),
        (
            CONNECTOR_RUNTIME_MEMORY_ACCESS,
            "CONNECTOR_RUNTIME_MEMORY_ACCESS",
        ),
        (
            CONNECTOR_RUNTIME_INPUT_OVERSIZED,
            "CONNECTOR_RUNTIME_INPUT_OVERSIZED",
        ),
        (
            CONNECTOR_RUNTIME_OUTPUT_OVERSIZED,
            "CONNECTOR_RUNTIME_OUTPUT_OVERSIZED",
        ),
        (
            CONNECTOR_RUNTIME_OUTPUT_INVALID,
            "CONNECTOR_RUNTIME_OUTPUT_INVALID",
        ),
        (
            CONNECTOR_RUNTIME_STATE_OVERSIZED,
            "CONNECTOR_RUNTIME_STATE_OVERSIZED",
        ),
        (
            CONNECTOR_RUNTIME_INSTANCE_UNUSABLE,
            "CONNECTOR_RUNTIME_INSTANCE_UNUSABLE",
        ),
        (
            CONNECTOR_RUNTIME_INPUT_INVALID,
            "CONNECTOR_RUNTIME_INPUT_INVALID",
        ),
        (
            CONNECTOR_RUNTIME_FIXTURE_INVALID,
            "CONNECTOR_RUNTIME_FIXTURE_INVALID",
        ),
        (
            CONNECTOR_RUNTIME_FIXTURE_MISMATCH,
            "CONNECTOR_RUNTIME_FIXTURE_MISMATCH",
        ),
        (CONNECTOR_HOST_STATE, "CONNECTOR_HOST_STATE"),
        (
            CONNECTOR_HOST_ACTION_INVALID,
            "CONNECTOR_HOST_ACTION_INVALID",
        ),
        (
            CONNECTOR_HOST_ACTION_UNDECLARED,
            "CONNECTOR_HOST_ACTION_UNDECLARED",
        ),
        (CONNECTOR_HOST_QUEUE_FULL, "CONNECTOR_HOST_QUEUE_FULL"),
        (
            CONNECTOR_HOST_RESULT_MISMATCH,
            "CONNECTOR_HOST_RESULT_MISMATCH",
        ),
        (
            CONNECTOR_HOST_SAMPLE_INVALID,
            "CONNECTOR_HOST_SAMPLE_INVALID",
        ),
        (CONNECTOR_HOST_LATE_RESULT, "CONNECTOR_HOST_LATE_RESULT"),
        (
            CONNECTOR_HOST_OPERATION_DUPLICATE,
            "CONNECTOR_HOST_OPERATION_DUPLICATE",
        ),
        (
            CONNECTOR_INSTALL_APPROVAL_INVALID,
            "CONNECTOR_INSTALL_APPROVAL_INVALID",
        ),
        (CONNECTOR_INSTALL_DOWNGRADE, "CONNECTOR_INSTALL_DOWNGRADE"),
        (CONNECTOR_INSTALL_NOT_FOUND, "CONNECTOR_INSTALL_NOT_FOUND"),
        (
            CONNECTOR_INSTALL_STATE_NAMESPACE,
            "CONNECTOR_INSTALL_STATE_NAMESPACE",
        ),
        (CONNECTOR_INSTALL_MIGRATION, "CONNECTOR_INSTALL_MIGRATION"),
        (CONNECTOR_INSTALL_STORAGE, "CONNECTOR_INSTALL_STORAGE"),
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
        assert_eq!(MavError::new(11_000, "x").category, Category::Connector);
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
