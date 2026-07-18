//! The WHOOP `DeviceCodec`. Layout-DSL packets delegate to the core's `ManifestCodec`; the two
//! packet kinds whose bodies the DSL cannot express — historical records (a version byte selects a
//! reviewed per-version layout) and events (a number byte selects a per-event body) — route to the
//! reviewed modules in this crate. The manifest still only *names* decoders; this codec is what
//! admits them, and the registry checks the names against [`Self::admitted lists`] at install.

use crate::{events, records};
use mav_codec::codec::{DeviceCodec, ManifestCodec};
use mav_codec::kv::DeviceKv;
use mav_codec::manifest::Manifest;
use mav_frame::frame::RawFrame;
use mav_model::error::Result;
use mav_model::raw::RawSample;

/// The codec id WHOOP manifests name in their `codec` field.
pub const CODEC_ID: &str = "whoop";

#[derive(Default, Debug, Clone)]
pub struct WhoopCodec {
    inner: ManifestCodec,
}

impl WhoopCodec {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DeviceCodec for WhoopCodec {
    fn decode(
        &mut self,
        frame: &RawFrame,
        manifest: &Manifest,
        kv: &mut dyn DeviceKv,
    ) -> Result<Vec<RawSample>> {
        let packet_type = frame.payload.first().copied();
        match packet_type.and_then(|t| manifest.packet_name(t)) {
            Some("historical_data") if !manifest.record_versions.is_empty() => {
                records::decode_record(manifest, &frame.payload)
            }
            Some("event") => match manifest.event_vocabulary.as_deref() {
                Some(vocabulary) => events::decode_event(vocabulary, &frame.payload),
                None => Ok(Vec::new()),
            },
            // Everything else — layouts, control packets, unknown types — keeps the core
            // manifest-driven semantics, including its typed errors.
            _ => self.inner.decode(frame, manifest, kv),
        }
    }

    fn admitted_record_decoders(&self) -> &'static [&'static str] {
        records::ADMITTED_DECODERS
    }

    fn admitted_event_vocabularies(&self) -> &'static [&'static str] {
        events::ADMITTED_EVENT_VOCABULARIES
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use mav_codec::kv::MemoryKv;
    use mav_model::raw::RawValue;
    use mav_model::stream::StreamKind;

    fn whoop_manifest() -> Manifest {
        Manifest::from_json(
            r#"{
                "schema": "connector-manifest/v1",
                "identity": { "family": "whoop5", "display_name": "WHOOP 5.0", "models": ["WHOOP 5.0"] },
                "gatt": { "service": "s", "command": "c", "notify": ["n"] },
                "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
                "codec": "whoop",
                "packets": { "35": "command", "47": "historical_data", "48": "event" },
                "record_versions": { "18": "r20_k18" },
                "event_vocabulary": "whoop",
                "capabilities": ["heart_rate", "battery_soc", "wrist_state"]
            }"#,
        )
        .unwrap()
    }

    fn decode(payload: Vec<u8>) -> Result<Vec<RawSample>> {
        let mut codec = WhoopCodec::new();
        let mut kv = MemoryKv::new();
        codec.decode(&RawFrame { payload }, &whoop_manifest(), &mut kv)
    }

    #[test]
    fn an_event_packet_routes_through_the_vocabulary() {
        // A wrist-on event: number at payload[2], RTC unix at payload[4..8].
        let mut payload = vec![48u8, 1, 9, 0, 0, 0, 0, 0];
        payload[4..8].copy_from_slice(&1_752_600_000u32.to_le_bytes());
        let samples = decode(payload).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].kind, StreamKind::WristState);
        assert_eq!(samples[0].value, RawValue::U8(1));
    }

    #[test]
    fn a_historical_record_routes_through_the_admitted_decoder() {
        // Inner record: [0]=type 47, [1]=version 18, [2]=command, then an r20_k18 body whose
        // HR byte at body[11] is 62.
        let mut payload = vec![0u8; 3 + records::R20_K18_MIN_BODY_LEN];
        payload[0] = 47;
        payload[1] = 18;
        payload[3 + 4..3 + 8].copy_from_slice(&1_752_600_000u32.to_le_bytes());
        payload[3 + 11] = 62;
        let samples = decode(payload).unwrap();
        assert_eq!(samples[0].kind, StreamKind::HeartRate);
        assert_eq!(samples[0].value, RawValue::U8(62));
    }

    #[test]
    fn a_control_packet_yields_no_samples_and_no_error() {
        assert_eq!(decode(vec![35u8, 1, 0, 0]).unwrap(), Vec::new());
    }

    #[test]
    fn the_codec_admits_its_reviewed_modules() {
        let codec = WhoopCodec::new();
        assert!(codec.admitted_record_decoders().contains(&"r20_k18"));
        assert_eq!(codec.admitted_event_vocabularies(), ["whoop"]);
    }
}
