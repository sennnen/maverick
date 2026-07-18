//! Public, device-neutral guest SDK for Maverick connectors.
//!
//! Connector crates implement [`Connector`] and invoke [`export_connector!`]. The SDK owns the
//! exact ABI symbols, deterministic CBOR boundary, allocation glue, bounded action construction,
//! and native test driver. Device protocol constants and host capabilities do not belong here.

mod builder;
mod driver;
mod export;
mod metadata;

pub use builder::ActionBuilder;
pub use driver::TestDriver;
pub use export::{ffi_alloc, ffi_dealloc, ffi_handle, ffi_init, ffi_snapshot, RuntimeCell};
pub use mav_connector_abi as abi;
pub use metadata::{ArtifactMetadata, EncodedMetadata};

use abi::{pack_ptr_len, ActionBatch, ConnectorEvent, WireError};
use std::fmt;

pub const ABI_VERSION: i64 = pack_ptr_len(1, 0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectorError {
    TooManyActions { limit: usize },
    InvalidWire(String),
    ReentrantCall,
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyActions { limit } => {
                write!(formatter, "connector emitted more than {limit} actions")
            }
            Self::InvalidWire(message) => write!(formatter, "connector wire error: {message}"),
            Self::ReentrantCall => formatter.write_str("connector ABI call is reentrant"),
        }
    }
}

impl std::error::Error for ConnectorError {}

impl From<WireError> for ConnectorError {
    fn from(value: WireError) -> Self {
        Self::InvalidWire(value.to_string())
    }
}

pub trait Connector: Default {
    fn init(&mut self, event: ConnectorEvent) -> Result<ActionBatch, ConnectorError> {
        self.handle(event)
    }

    fn handle(&mut self, event: ConnectorEvent) -> Result<ActionBatch, ConnectorError>;

    fn snapshot(&self) -> Result<Vec<u8>, ConnectorError>;
}

#[macro_export]
macro_rules! export_connector {
    ($connector:ty) => {
        static MAV_CONNECTOR: $crate::RuntimeCell<$connector> = $crate::RuntimeCell::new();

        #[no_mangle]
        pub extern "C" fn mav_abi_version() -> i64 {
            $crate::ABI_VERSION
        }

        #[no_mangle]
        pub extern "C" fn mav_alloc(len: i32) -> i32 {
            $crate::ffi_alloc(len)
        }

        #[no_mangle]
        pub extern "C" fn mav_dealloc(ptr: i32, len: i32) {
            $crate::ffi_dealloc(ptr, len);
        }

        #[no_mangle]
        pub extern "C" fn mav_init(ptr: i32, len: i32) -> i64 {
            $crate::ffi_init(&MAV_CONNECTOR, ptr, len)
        }

        #[no_mangle]
        pub extern "C" fn mav_handle(ptr: i32, len: i32) -> i64 {
            $crate::ffi_handle(&MAV_CONNECTOR, ptr, len)
        }

        #[no_mangle]
        pub extern "C" fn mav_snapshot() -> i64 {
            $crate::ffi_snapshot(&MAV_CONNECTOR)
        }
    };
}

#[macro_export]
macro_rules! artifact_metadata {
    (
        $visibility:vis fn $name:ident() {
            manifest: $manifest:expr,
            abi: $abi:expr,
            fixtures: $fixtures:expr $(,)?
        }
    ) => {
        $visibility fn $name() -> Result<$crate::ArtifactMetadata, $crate::ConnectorError> {
            Ok($crate::ArtifactMetadata::new($manifest, $abi, $fixtures))
        }
    };
}
