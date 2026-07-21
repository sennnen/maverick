//! Normalized sample admission for declarative layouts and open Bluetooth standard profiles.

use crate::manifest::{Conversion, FieldType, Layout, Manifest, SubsecondsUnit, TimeSpec};
use mav_frame::frame::RawFrame;
use mav_frame::reader::TypedReader;
use mav_model::error::{codes, MavError, Result};
use mav_model::raw::{RawSample, RawValue};
use mav_model::time::DeviceTime;

/// Pure interpretation of normalized layouts. Its only state is the session-monotonic sequence
/// for standard-profile samples, which carry no device clock of their own.
#[derive(Default, Debug, Clone)]
pub struct SampleAdmission {
    standard_seq: u16,
}

impl SampleAdmission {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decode(&mut self, frame: &RawFrame, manifest: &Manifest) -> Result<Vec<RawSample>> {
        if let Some(profile) = manifest.standard_profile.as_deref() {
            return crate::standard::decode_standard_profile(
                profile,
                &frame.payload,
                &mut self.standard_seq,
            );
        }
        let reader = TypedReader::new(&frame.payload);
        let packet_type = reader
            .u8_at(0)
            .map_err(|e| e.context("reading packet type"))?;
        let layout = match manifest.layout_for_packet(packet_type)? {
            Some(layout) => layout,
            None => {
                // A layout-less known packet is control or intentionally not decoded.
                return Ok(Vec::new());
            }
        };
        decode_layout(&reader, layout, packet_type)
    }
}

fn decode_layout(
    reader: &TypedReader<'_>,
    layout: &Layout,
    packet_type: u8,
) -> Result<Vec<RawSample>> {
    let device_time = match &layout.time {
        Some(spec) => read_time(reader, spec, packet_type)?,
        None => DeviceTime::from_nanos(0),
    };

    let mut samples = Vec::new();
    for field in &layout.fields {
        let value = read_value(reader, field.field_type, field.offset, field.conversion)
            .map_err(|e| field_error(e, &field.name, packet_type))?;
        if let Some(stream) = field.stream {
            samples.push(RawSample {
                kind: stream,
                device_time,
                seq: 0,
                value,
            });
        }
    }

    for repeat in &layout.repeats {
        let count = usize::from(
            reader
                .u8_at(repeat.count_offset)
                .map_err(|e| field_error(e, &repeat.name, packet_type))?,
        );
        if count > repeat.max_count {
            return Err(MavError::new(
                codes::DECODE_FIELD_UNREADABLE,
                "repeat count exceeds the layout's sanity bound",
            )
            .context(format!(
                "{} count {count} > max {} in packet 0x{packet_type:02x}",
                repeat.name, repeat.max_count
            )));
        }
        let mut seq: u16 = 0;
        for slot in 0..count {
            let offset = repeat.start_offset + slot * repeat.stride;
            let value = read_value(reader, repeat.field_type, offset, repeat.conversion)
                .map_err(|e| field_error(e, &repeat.name, packet_type))?;
            if repeat.drop_zero && value.key_bits() == 0 {
                continue;
            }
            samples.push(RawSample {
                kind: repeat.stream,
                device_time,
                seq,
                value,
            });
            seq += 1;
        }
    }

    Ok(samples)
}

fn read_time(reader: &TypedReader<'_>, spec: &TimeSpec, packet_type: u8) -> Result<DeviceTime> {
    let seconds = reader
        .u32_le_at(spec.seconds_offset)
        .map_err(|e| field_error(e, "timestamp seconds", packet_type))?;
    let mut nanos = i64::from(seconds) * 1_000_000_000;
    if let Some(offset) = spec.subseconds_offset {
        let subseconds = reader
            .u16_le_at(offset)
            .map_err(|e| field_error(e, "timestamp subseconds", packet_type))?;
        nanos += match spec.subseconds_unit {
            SubsecondsUnit::Milliseconds => i64::from(subseconds) * 1_000_000,
            SubsecondsUnit::Ticks32768 => i64::from(subseconds) * 1_000_000_000 / 32_768,
        };
    }
    Ok(DeviceTime::from_nanos(nanos))
}

fn read_value(
    reader: &TypedReader<'_>,
    field_type: FieldType,
    offset: usize,
    conversion: Option<Conversion>,
) -> Result<RawValue> {
    let raw = match field_type {
        FieldType::U8 => RawValue::U8(reader.u8_at(offset)?),
        FieldType::U16Le => RawValue::U16(reader.u16_le_at(offset)?),
        FieldType::U32Le => RawValue::U32(reader.u32_le_at(offset)?),
        FieldType::I16Le => RawValue::I16(reader.i16_le_at(offset)?),
        FieldType::I32Le => RawValue::I32(reader.i32_le_at(offset)?),
        FieldType::F32Le => RawValue::F32(reader.f32_le_at(offset)?),
    };
    Ok(match conversion {
        Some(c) => RawValue::Converted(raw.as_f64() * c.scale + c.offset),
        None => raw,
    })
}

fn field_error(e: MavError, field: &str, packet_type: u8) -> MavError {
    MavError::new(codes::DECODE_FIELD_UNREADABLE, "field could not be read")
        .context(format!("field {field} in packet 0x{packet_type:02x}"))
        .context(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;
    use mav_model::stream::StreamKind;

    /// A realtime-shaped manifest matching the gen5 REALTIME_DATA layout from the ledger, offsets
    /// payload-relative (payload = the inner record, payload[0] = packet type) as the [WRS] real
    /// captures pin them: ts u32 @ 2, subsec u16 @ 6, HR u8 @ 8, rr_count u8 @ 9, rr u16 LE from
    /// 10 stride 2.
    fn realtime_manifest() -> Manifest {
        Manifest::from_json(
            r#"{
                "schema": "connector-manifest/v1",
                "identity": {
                    "family": "testgen5",
                    "display_name": "Test gen5",
                    "models": ["TEST 5"]
                },
                "gatt": { "service": "s", "command": "c", "notify": ["n"] },
                "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
                "packets": { "35": "command", "40": "realtime_data" },
                "layouts": {
                    "realtime_data": {
                        "time": {
                            "seconds_offset": 2,
                            "subseconds_offset": 6,
                            "subseconds_unit": "milliseconds"
                        },
                        "fields": [
                            { "name": "heart_rate", "stream": "heart_rate", "type": "u8", "offset": 8 }
                        ],
                        "repeats": [
                            {
                                "name": "rr_interval", "stream": "rr_interval", "type": "u16_le",
                                "count_offset": 9, "start_offset": 10, "stride": 2,
                                "max_count": 16, "drop_zero": true
                            }
                        ]
                    }
                },
                "capabilities": ["heart_rate", "rr_interval"]
            }"#,
        )
        .unwrap()
    }

    /// payload[0]=type 40, [1]=seq, then the fields at their documented offsets.
    fn realtime_payload(hr: u8, rrs: &[u16]) -> Vec<u8> {
        let mut p = vec![0u8; 10 + rrs.len() * 2];
        p[0] = 40;
        p[1] = 1;
        p[2..6].copy_from_slice(&1_752_600_000u32.to_le_bytes());
        p[6..8].copy_from_slice(&250u16.to_le_bytes());
        p[8] = hr;
        p[9] = rrs.len() as u8;
        for (i, rr) in rrs.iter().enumerate() {
            p[10 + i * 2..12 + i * 2].copy_from_slice(&rr.to_le_bytes());
        }
        p
    }

    fn decode(payload: Vec<u8>) -> Result<Vec<RawSample>> {
        let mut admission = SampleAdmission::new();
        admission.decode(&RawFrame { payload }, &realtime_manifest())
    }

    #[test]
    fn decodes_hr_and_rr_with_device_time() {
        let samples = decode(realtime_payload(62, &[812, 812, 790])).unwrap();
        assert_eq!(samples.len(), 4);

        let hr = &samples[0];
        assert_eq!(hr.kind, StreamKind::HeartRate);
        assert_eq!(hr.value, RawValue::U8(62));
        assert_eq!(
            hr.device_time,
            DeviceTime::from_nanos(1_752_600_000 * 1_000_000_000 + 250 * 1_000_000)
        );

        let rr_values: Vec<_> = samples[1..].iter().map(|s| s.value).collect();
        assert_eq!(
            rr_values,
            vec![RawValue::U16(812), RawValue::U16(812), RawValue::U16(790)]
        );
    }

    #[test]
    fn equal_rr_values_in_one_packet_carry_distinct_seq() {
        let samples = decode(realtime_payload(60, &[812, 812])).unwrap();
        let seqs: Vec<_> = samples[1..].iter().map(|s| s.seq).collect();
        assert_eq!(
            seqs,
            vec![0, 1],
            "two equal RR intervals are two distinct beats"
        );
    }

    #[test]
    fn zero_ms_rr_placeholders_are_dropped_without_error() {
        let samples = decode(realtime_payload(60, &[812, 0, 790])).unwrap();
        let rr_values: Vec<_> = samples[1..].iter().map(|s| s.value).collect();
        assert_eq!(rr_values, vec![RawValue::U16(812), RawValue::U16(790)]);
    }

    #[test]
    fn control_packet_yields_no_samples_and_no_error() {
        let mut payload = realtime_payload(60, &[]);
        payload[0] = 35;
        assert_eq!(decode(payload).unwrap(), Vec::new());
    }

    #[test]
    fn unknown_packet_type_is_a_typed_error() {
        let mut payload = realtime_payload(60, &[]);
        payload[0] = 99;
        let err = decode(payload).unwrap_err();
        assert_eq!(err.code, codes::DECODE_UNKNOWN_PACKET_TYPE);
    }

    #[test]
    fn runaway_repeat_count_is_bounded() {
        let mut payload = realtime_payload(60, &[812]);
        payload[9] = 200;
        let err = decode(payload).unwrap_err();
        assert_eq!(err.code, codes::DECODE_FIELD_UNREADABLE);
        assert!(err.to_string().contains("max 16"), "{err}");
    }

    #[test]
    fn truncated_payload_is_a_typed_error_not_a_panic() {
        let err = decode(vec![40, 1, 0, 0]).unwrap_err();
        assert_eq!(err.code, codes::DECODE_FIELD_UNREADABLE);
    }

    #[test]
    fn a_layoutless_packet_yields_no_samples() {
        let manifest = Manifest::from_json(
            r#"{
                "schema": "connector-manifest/v1",
                "identity": { "family": "t", "display_name": "T", "models": ["T"] },
                "gatt": { "service": "s", "command": "c", "notify": ["n"] },
                "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
                "packets": { "48": "event" },
                "capabilities": ["heart_rate"]
            }"#,
        )
        .unwrap();
        let payload = vec![48u8, 1, 9, 0, 0, 0, 0, 0];
        let mut admission = SampleAdmission::new();
        let samples = admission.decode(&RawFrame { payload }, &manifest).unwrap();
        assert_eq!(samples, Vec::new());
    }

    #[test]
    fn unit_conversion_applies_scale_and_offset() {
        let manifest = Manifest::from_json(
            r#"{
                "schema": "connector-manifest/v1",
                "identity": { "family": "t", "display_name": "T", "models": ["T"] },
                "gatt": { "service": "s", "command": "c", "notify": ["n"] },
                "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
                "packets": { "47": "temp" },
                "layouts": {
                    "temp": {
                        "fields": [
                            {
                                "name": "skin_temp", "stream": "skin_temp", "type": "u16_le",
                                "offset": 3, "conversion": { "scale": 0.01 }
                            }
                        ]
                    }
                },
                "capabilities": ["skin_temp"]
            }"#,
        )
        .unwrap();

        let mut payload = vec![47u8, 0, 0, 0, 0];
        payload[3..5].copy_from_slice(&3650u16.to_le_bytes());
        let mut admission = SampleAdmission::new();
        let samples = admission.decode(&RawFrame { payload }, &manifest).unwrap();
        assert_eq!(samples[0].value, RawValue::Converted(36.5));
    }
}
