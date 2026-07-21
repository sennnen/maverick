//! Normalized sample admission for declarative layouts and explicitly admitted open Bluetooth SIG
//! profiles. Device protocol implementations live only in signed `.mavconn` artifacts.
#![forbid(unsafe_code)]

pub mod codec;
pub mod manifest;
pub mod standard;

pub use codec::SampleAdmission;
pub use manifest::{
    CommandSpec, Conversion, EnableFlag, EnableSequence, FieldSpec, FieldType, FrameConfig,
    FrameSpecConfig, Gatt, GattProfile, HeaderCrcConfig, Identity, IntervalSourceConfig, Layout,
    LengthFieldConfig, Manifest, RepeatSpec, StandardGatt, SubsecondsUnit, TimeSpec, TrailerConfig,
};
