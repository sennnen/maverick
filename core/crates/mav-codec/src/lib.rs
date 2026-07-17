//! The connector contract: manifest types, the boxed `DeviceCodec` trait, the per-device
//! key-value store handle, the manifest-driven default codec, and the registry that resolves a
//! model string to a device family. The design and its boundaries are docs/connectors.md; the
//! short form is that a manifest holds everything static, a codec holds only what data cannot
//! express, and a codec's interface gives it no way to reach storage, the network, analytics, or
//! another device.
#![forbid(unsafe_code)]

pub mod codec;
pub mod control;
pub mod kv;
pub mod manifest;
pub mod registry;

pub use codec::{DeviceCodec, ManifestCodec};
pub use kv::{DeviceKv, MemoryKv};
pub use manifest::{
    CommandSpec, Conversion, EnableFlag, EnableSequence, FieldSpec, FieldType, FrameConfig,
    FrameSpecConfig, Gatt, GattProfile, HeaderCrcConfig, Identity, IntervalSourceConfig, Layout,
    LengthFieldConfig, Manifest, RepeatSpec, StandardGatt, SubsecondsUnit, TimeSpec, TrailerConfig,
};
pub use registry::Registry;
