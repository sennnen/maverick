//! The WHOOP device codec: everything WHOOP-specific that the manifest DSL cannot express, boxed
//! behind the `DeviceCodec` contract so the core never learns a WHOOP fact (ADR-016). The modules
//! here are the reviewed decoders the manifest can only name — historical record versions, the
//! event vocabulary, the historical-control layouts — plus the outbound command builders. Wire
//! facts and their confidence tags live in docs/protocol/whoop.md.

#![forbid(unsafe_code)]

pub mod codec;
pub mod commands;
pub mod control;
pub mod events;
pub mod records;

pub use codec::WhoopCodec;
