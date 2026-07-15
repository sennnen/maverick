//! Opaque identifiers. Each is a newtype over `u64` so that a device id can never be passed where
//! a stream id is expected, and so that the walk-back from a metric to the bytes it came from is a
//! chain of typed keys rather than a pile of bare integers.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}#{}", stringify!($name), self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl From<u64> for $name {
            fn from(value: u64) -> Self {
                Self(value)
            }
        }
    };
}

define_id!(
    /// A physical wearable across all of its sessions.
    DeviceId
);
define_id!(
    /// One continuous capture period for a device.
    SessionId
);
define_id!(
    /// A time-ordered typed stream of samples.
    StreamId
);
define_id!(
    /// A single reassembled, CRC-checked BLE frame.
    FrameId
);
define_id!(
    /// A row in the provenance table describing how a feature was produced.
    MetadataId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_returns_inner_value() {
        assert_eq!(DeviceId::new(42).get(), 42);
    }

    #[test]
    fn display_and_debug_differ() {
        let id = StreamId::new(7);
        assert_eq!(format!("{id}"), "StreamId#7");
        assert_eq!(format!("{id:?}"), "StreamId(7)");
    }

    #[test]
    fn serde_roundtrips_as_bare_number() {
        let id = FrameId::new(900);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "900");
        assert_eq!(serde_json::from_str::<FrameId>(&json).unwrap(), id);
    }
}
