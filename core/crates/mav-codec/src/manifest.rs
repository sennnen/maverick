//! Manifest types: the static description of one device family, deserialised from
//! `connectors/<device>/manifest.json`. Field meanings and the schema rationale are in
//! docs/connectors.md. Everything here is data about the wire; analytic judgement (thresholds,
//! baselines) does not belong in a manifest.

use mav_model::error::{codes, MavError, Result};
use mav_model::stream::StreamKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MANIFEST_SCHEMA: &str = "connector-manifest/v1";

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub identity: Identity,
    pub gatt: Gatt,
    /// Standard GATT profiles the device also exposes (heart rate, battery, device info). On some
    /// devices these are readable without the custom-service bond, which is how a live pulse comes
    /// out of a WHOOP 5.0 with no handshake at all.
    #[serde(default)]
    pub standard_gatt: Option<StandardGatt>,
    pub frame: FrameConfig,
    /// The command opcodes the acquisition state machine writes. Data, not logic: the machine
    /// reads this table, the manifest does not sequence anything.
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
    /// The feature-flag writes that turn a mute strap into a streaming one. Present only on
    /// devices that gate their data behind a configuration handshake (the gen5 straps do).
    #[serde(default)]
    pub enable_sequence: Option<EnableSequence>,
    /// Packet-type byte to layout key. A key present in `layouts` decodes to samples; a key
    /// absent from `layouts` is a known control or not-yet-decoded packet that produces no
    /// samples and no error. A packet-type byte missing from this map entirely is unknown and is
    /// logged as such.
    pub packets: BTreeMap<u8, String>,
    #[serde(default)]
    pub layouts: BTreeMap<String, Layout>,
    /// Historical record version byte to layout key; filled in from M5.
    #[serde(default)]
    pub record_versions: BTreeMap<u8, String>,
    /// The stream kinds this device can produce. Capability negotiation intersects these with
    /// what each analytic requires.
    pub capabilities: Vec<StreamKind>,
    #[serde(default)]
    pub confidence_note: String,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub family: String,
    pub display_name: String,
    /// Model strings that resolve to this family, compared after trimming, case-insensitively.
    pub models: Vec<String>,
    /// True on the family that catches model strings nothing else claims (the ledger rule:
    /// unknown or legacy strings default to gen5).
    #[serde(default)]
    pub fallback_for_unknown_models: bool,
    #[serde(default)]
    pub confidence: String,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gatt {
    pub service: String,
    pub command: String,
    pub notify: Vec<String>,
}

/// Standard-profile characteristics, stored as their 16-bit assigned numbers (e.g. "180D"). These
/// are the Bluetooth SIG profiles any client can read; the point of listing them is that they can
/// be a data source that sidesteps the custom service entirely.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardGatt {
    #[serde(default)]
    pub heart_rate: Option<GattProfile>,
    #[serde(default)]
    pub battery: Option<GattProfile>,
    /// Device Information service UUID, if present. Its model-string characteristic is one way to
    /// tell apart devices that share a custom-service UUID at scan time.
    #[serde(default)]
    pub device_info_service: Option<String>,
    /// True when the heart-rate profile answers before the custom-service bond is established.
    #[serde(default)]
    pub heart_rate_unbonded: bool,
    #[serde(default)]
    pub confidence: String,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GattProfile {
    pub service: String,
    pub characteristic: String,
}

/// One command the state machine can send. `b3` is the fourth inner byte, which is
/// command-specific on the gen5 wire and which the strap checks before it will act; a wrong `b3`
/// is ignored with no error, so it is recorded here rather than guessed at the call site. Left
/// absent on devices that do not use the convention.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub name: String,
    pub opcode: u8,
    #[serde(default)]
    pub b3: Option<u8>,
    #[serde(default)]
    pub note: String,
}

/// The configuration handshake: a run of writes through one command, each carrying a feature-flag
/// name in a fixed-width field followed by a value byte. On the gen5 straps this is the step that
/// actually unlocks the biometric stream.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnableSequence {
    /// The name of the command in `commands` that performs each write.
    pub command: String,
    /// Width of the ASCII flag-name field at the start of the payload.
    pub name_field_bytes: usize,
    /// Total payload length (name field, then the value byte, then padding).
    pub payload_bytes: usize,
    pub flags: Vec<EnableFlag>,
    #[serde(default)]
    pub confidence: String,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnableFlag {
    pub name: String,
    pub value: u8,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameConfig {
    /// "gen4" or "gen5"; must name a wire format mav-frame implements.
    pub wire_format: String,
    pub max_frame_bytes: u32,
}

/// How to decode one packet kind's payload into samples. Offsets are relative to the start of the
/// inner payload (`payload[0]` is the packet-type byte).
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default)]
    pub time: Option<TimeSpec>,
    #[serde(default)]
    pub fields: Vec<FieldSpec>,
    #[serde(default)]
    pub repeats: Vec<RepeatSpec>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeSpec {
    /// Offset of a u32 little-endian unix-seconds field.
    pub seconds_offset: usize,
    #[serde(default)]
    pub subseconds_offset: Option<usize>,
    #[serde(default)]
    pub subseconds_unit: SubsecondsUnit,
    #[serde(default)]
    pub confidence: String,
}

/// The wire does not say what a subseconds field counts; the two candidates observed in the
/// ledger are milliseconds and 1/32768 ticks, and the manifest must pick one per field.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubsecondsUnit {
    #[default]
    Milliseconds,
    Ticks32768,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    pub name: String,
    /// Absent for fields decoded but not emitted as samples (status words and the like).
    #[serde(default)]
    pub stream: Option<StreamKind>,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    pub offset: usize,
    #[serde(default)]
    pub conversion: Option<Conversion>,
}

/// A counted group of equal-width values, like the RR-interval slots in a realtime packet.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatSpec {
    pub name: String,
    pub stream: StreamKind,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Offset of the u8 count field.
    pub count_offset: usize,
    pub start_offset: usize,
    pub stride: usize,
    /// A sanity bound on the count field; a wire count above this is a decode error, not a loop.
    pub max_count: usize,
    /// Zero raw values are expected padding in some layouts (0 ms RR slots); drop them without
    /// logging when true.
    #[serde(default)]
    pub drop_zero: bool,
    #[serde(default)]
    pub conversion: Option<Conversion>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    U8,
    U16Le,
    U32Le,
    I16Le,
    I32Le,
    F32Le,
}

/// A fixed linear unit conversion: physical = raw * scale + offset. Anything beyond that (learned
/// anchors, stateful decode) is codec territory, per docs/connectors.md.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conversion {
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
}

impl Manifest {
    pub fn from_json(json: &str) -> Result<Self> {
        let manifest: Manifest = serde_json::from_str(json).map_err(|e| {
            MavError::new(codes::DECODE_LAYOUT_INVALID, "manifest does not parse")
                .context(e.to_string())
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != MANIFEST_SCHEMA {
            return Err(
                MavError::new(codes::DECODE_LAYOUT_INVALID, "unsupported manifest schema")
                    .context(format!("got {:?}, want {MANIFEST_SCHEMA:?}", self.schema)),
            );
        }
        if !matches!(self.frame.wire_format.as_str(), "gen4" | "gen5") {
            return Err(
                MavError::new(codes::DECODE_LAYOUT_INVALID, "unknown wire_format")
                    .context(self.frame.wire_format.clone()),
            );
        }
        if self.identity.models.is_empty() {
            return Err(MavError::new(
                codes::DECODE_LAYOUT_INVALID,
                "identity.models must not be empty",
            ));
        }
        for layout_key in self.record_versions.values() {
            if !self.layouts.contains_key(layout_key) {
                return Err(MavError::new(
                    codes::DECODE_LAYOUT_INVALID,
                    "record_versions references a missing layout",
                )
                .context(layout_key.clone()));
            }
        }
        for (key, layout) in &self.layouts {
            for repeat in &layout.repeats {
                if repeat.stride == 0 || repeat.max_count == 0 {
                    return Err(MavError::new(
                        codes::DECODE_LAYOUT_INVALID,
                        "repeat stride and max_count must be positive",
                    )
                    .context(format!("layout {key}, repeat {}", repeat.name)));
                }
            }
        }
        if let Some(sequence) = &self.enable_sequence {
            if !self.commands.iter().any(|c| c.name == sequence.command) {
                return Err(MavError::new(
                    codes::DECODE_LAYOUT_INVALID,
                    "enable_sequence.command names no command in the manifest",
                )
                .context(sequence.command.clone()));
            }
            if sequence.name_field_bytes >= sequence.payload_bytes {
                return Err(MavError::new(
                    codes::DECODE_LAYOUT_INVALID,
                    "enable_sequence payload has no room for a value byte after the name field",
                )
                .context(format!(
                    "name_field {} >= payload {}",
                    sequence.name_field_bytes, sequence.payload_bytes
                )));
            }
        }
        Ok(())
    }

    pub fn command(&self, name: &str) -> Option<&CommandSpec> {
        self.commands.iter().find(|c| c.name == name)
    }

    /// The layout for a packet-type byte: `Ok(Some)` when it decodes to samples, `Ok(None)` for a
    /// known control packet, `Err` for a byte the manifest has never heard of.
    pub fn layout_for_packet(&self, packet_type: u8) -> Result<Option<&Layout>> {
        match self.packets.get(&packet_type) {
            None => Err(MavError::new(
                codes::DECODE_UNKNOWN_PACKET_TYPE,
                "packet type not in manifest packet map",
            )
            .context(format!("type {packet_type} (0x{packet_type:02x})"))),
            Some(key) => Ok(self.layouts.get(key)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_json() -> String {
        r#"{
            "schema": "connector-manifest/v1",
            "identity": {
                "family": "test",
                "display_name": "Test Strap",
                "models": ["TEST 1"]
            },
            "gatt": { "service": "0000", "command": "0001", "notify": ["0002"] },
            "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
            "packets": { "35": "command", "40": "realtime" },
            "layouts": {
                "realtime": {
                    "fields": [
                        { "name": "hr", "stream": "heart_rate", "type": "u8", "offset": 16 }
                    ]
                }
            },
            "capabilities": ["heart_rate"]
        }"#
        .to_owned()
    }

    #[test]
    fn minimal_manifest_parses_and_validates() {
        let manifest = Manifest::from_json(&minimal_json()).unwrap();
        assert_eq!(manifest.identity.family, "test");
        assert_eq!(manifest.capabilities, vec![StreamKind::HeartRate]);
    }

    #[test]
    fn wrong_schema_is_refused() {
        let json = minimal_json().replace("connector-manifest/v1", "connector-manifest/v9");
        let err = Manifest::from_json(&json).unwrap_err();
        assert_eq!(err.code, codes::DECODE_LAYOUT_INVALID);
    }

    #[test]
    fn unknown_wire_format_is_refused() {
        let json = minimal_json().replace("gen5", "gen9");
        let err = Manifest::from_json(&json).unwrap_err();
        assert_eq!(err.code, codes::DECODE_LAYOUT_INVALID);
    }

    #[test]
    fn unknown_top_level_field_is_refused() {
        let json = minimal_json().replace("\"capabilities\"", "\"surprise\": 1, \"capabilities\"");
        assert!(Manifest::from_json(&json).is_err());
    }

    #[test]
    fn commands_and_enable_sequence_parse_and_are_looked_up() {
        let json = minimal_json().replace(
            "\"capabilities\"",
            r#""commands": [
                { "name": "get_hello", "opcode": 145, "b3": 1 },
                { "name": "set_config", "opcode": 120, "b3": 1 },
                { "name": "get_data_range", "opcode": 34, "b3": 0 }
            ],
            "enable_sequence": {
                "command": "set_config",
                "name_field_bytes": 32,
                "payload_bytes": 40,
                "flags": [{ "name": "enable_r22_packets", "value": 1 }]
            },
            "capabilities""#,
        );
        let manifest = Manifest::from_json(&json).unwrap();
        assert_eq!(manifest.command("get_hello").unwrap().opcode, 145);
        assert_eq!(manifest.command("get_data_range").unwrap().b3, Some(0));
        assert!(manifest.command("nonexistent").is_none());
        let seq = manifest.enable_sequence.as_ref().unwrap();
        assert_eq!(seq.command, "set_config");
        assert_eq!(seq.flags[0].name, "enable_r22_packets");
    }

    #[test]
    fn enable_sequence_naming_an_absent_command_is_refused() {
        let json = minimal_json().replace(
            "\"capabilities\"",
            r#""enable_sequence": {
                "command": "set_config",
                "name_field_bytes": 32,
                "payload_bytes": 40,
                "flags": []
            },
            "capabilities""#,
        );
        let err = Manifest::from_json(&json).unwrap_err();
        assert_eq!(err.code, codes::DECODE_LAYOUT_INVALID);
    }

    #[test]
    fn enable_sequence_with_no_room_for_a_value_byte_is_refused() {
        let json = minimal_json().replace(
            "\"capabilities\"",
            r#""commands": [{ "name": "set_config", "opcode": 120, "b3": 1 }],
            "enable_sequence": {
                "command": "set_config",
                "name_field_bytes": 40,
                "payload_bytes": 40,
                "flags": []
            },
            "capabilities""#,
        );
        let err = Manifest::from_json(&json).unwrap_err();
        assert_eq!(err.code, codes::DECODE_LAYOUT_INVALID);
    }

    #[test]
    fn standard_gatt_parses_when_present() {
        let json = minimal_json().replace(
            "\"capabilities\"",
            r#""standard_gatt": {
                "heart_rate": { "service": "180D", "characteristic": "2A37" },
                "battery": { "service": "180F", "characteristic": "2A19" },
                "device_info_service": "180A",
                "heart_rate_unbonded": true
            },
            "capabilities""#,
        );
        let manifest = Manifest::from_json(&json).unwrap();
        let sg = manifest.standard_gatt.as_ref().unwrap();
        assert_eq!(sg.heart_rate.as_ref().unwrap().characteristic, "2A37");
        assert!(sg.heart_rate_unbonded);
    }

    #[test]
    fn layout_lookup_distinguishes_data_control_and_unknown() {
        let manifest = Manifest::from_json(&minimal_json()).unwrap();
        assert!(manifest.layout_for_packet(40).unwrap().is_some());
        assert!(manifest.layout_for_packet(35).unwrap().is_none());
        let err = manifest.layout_for_packet(99).unwrap_err();
        assert_eq!(err.code, codes::DECODE_UNKNOWN_PACKET_TYPE);
    }
}
