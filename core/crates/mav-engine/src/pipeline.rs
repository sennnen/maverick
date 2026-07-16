//! The synchronous pipeline for the realtime slice: a capture of raw notification bytes in, a
//! snapshot out, running decode, signal quality, timeline, storage, feature, and snapshot in the
//! fixed order docs/pipeline.md sets. It is the shared path `mav-replay` and the FFI both drive, so
//! that a capture replayed offline runs the identical code the live device would.

use crate::snapshot::Snapshot;
use mav_codec::codec::{DeviceCodec, ManifestCodec};
use mav_codec::kv::MemoryKv;
use mav_codec::manifest::Manifest;
use mav_feature::hr::{hr_summary, HR_FEATURE_ALGORITHM, HR_FEATURE_VERSION};
use mav_frame::frame::WireFormat;
use mav_frame::reassembler::{Reassembler, ReassemblyEvent};
use mav_model::error::{codes, MavError, Result};
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::RawSampleBatch;
use mav_model::stream::StreamKind;
use mav_model::time::WallTime;
use mav_obs::stage::Stage;
use mav_obs::tap::{Ids, Tap, TapEvent};
use mav_store::{Provenance, Store};
use mav_timeline::{place_on_wall, Timeline};

/// Fixed provenance ids for the M1 slice, so the snapshot is byte-for-byte reproducible.
const SQI_PROVENANCE: MetadataId = MetadataId::new(1);
const HR_PROVENANCE: MetadataId = MetadataId::new(2);

/// A capture of raw notification bytes to replay through the pipeline. The chunks are exactly what
/// the radio delivered, so a real sniffed capture and a synthetic one run the same way.
#[derive(Clone, PartialEq, Debug)]
pub struct Capture {
    pub device: DeviceId,
    /// The wall time the phone received the capture, used only when a device timestamp is
    /// implausible and the timeline has to fall back.
    pub capture_wall: WallTime,
    pub chunks: Vec<Vec<u8>>,
}

/// Run one realtime capture through the pipeline against a device manifest, writing into `store`
/// and returning the snapshot. The store is the caller's, so a replay and a live session share the
/// same storage model.
pub fn run_realtime(
    manifest: &Manifest,
    capture: &Capture,
    store: &Store,
    tap: &dyn Tap,
) -> Result<Snapshot> {
    let wire = wire_format(manifest)?;
    let mut reassembler = Reassembler::new(wire);
    let mut codec = ManifestCodec::new();
    let mut kv = MemoryKv::new();
    let mut timeline = Timeline::new();
    let ids = Ids {
        device: Some(capture.device),
        ..Ids::default()
    };

    for chunk in &capture.chunks {
        for event in reassembler.push(chunk) {
            let frame = match event {
                ReassemblyEvent::Frame(frame) => frame,
                ReassemblyEvent::InvalidFrame(error) => {
                    tap.on_stage(Stage::Acquisition, TapEvent::Rejected { error, ids });
                    continue;
                }
                ReassemblyEvent::SkippedGarbage { bytes } => {
                    let error = MavError::warning(
                        codes::FRAME_GARBAGE_SKIPPED,
                        "bytes discarded while resynchronising",
                    )
                    .context(format!("{bytes} bytes"));
                    tap.on_stage(Stage::Acquisition, TapEvent::Rejected { error, ids });
                    continue;
                }
            };

            let decoded = codec.decode(&frame, manifest, &mut kv)?;
            if decoded.is_empty() {
                continue;
            }
            tap.on_stage(
                Stage::Decode,
                TapEvent::Produced {
                    count: decoded.len(),
                    ids,
                    summary: None,
                },
            );

            let batch = RawSampleBatch {
                device: capture.device,
                samples: decoded,
            };
            let scored = mav_sqi::score_batch(&batch, SQI_PROVENANCE);
            tap.on_stage(
                Stage::Sqi,
                TapEvent::Produced {
                    count: scored.len(),
                    ids,
                    summary: None,
                },
            );

            for mut sample in scored {
                place_on_wall(&mut sample, capture.capture_wall);
                timeline.insert(sample);
            }
        }
    }

    let mut stored = 0usize;
    for sample in timeline.drain_ordered() {
        store.insert_sample(capture.device, &sample)?;
        stored += 1;
    }
    if stored > 0 {
        tap.on_stage(
            Stage::Store,
            TapEvent::Produced {
                count: stored,
                ids,
                summary: None,
            },
        );
    }

    let hr_samples = store.samples(capture.device, StreamKind::HeartRate)?;
    let summary = hr_summary(&hr_samples, HR_PROVENANCE);
    tap.on_stage(
        Stage::Features,
        TapEvent::Produced {
            count: 1,
            ids,
            summary: None,
        },
    );

    store.upsert_provenance(&Provenance {
        metadata: HR_PROVENANCE,
        source_stream: StreamKind::HeartRate,
        quality: 1.0,
        algorithm_id: HR_FEATURE_ALGORITHM.to_owned(),
        algorithm_version: HR_FEATURE_VERSION,
        sample_count: summary.sample_count,
    })?;

    let snapshot = Snapshot::from_hr(capture.device, &summary);
    tap.on_stage(
        Stage::Snapshots,
        TapEvent::Produced {
            count: 1,
            ids,
            summary: None,
        },
    );
    Ok(snapshot)
}

fn wire_format(manifest: &Manifest) -> Result<WireFormat> {
    match manifest.frame.wire_format.as_str() {
        "gen4" => Ok(WireFormat::Gen4),
        "gen5" => Ok(WireFormat::Gen5),
        other => Err(MavError::new(
            codes::DECODE_LAYOUT_INVALID,
            "manifest wire_format is not one mav-frame implements",
        )
        .context(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_frame::frame::build_frame;

    fn realtime_manifest() -> Manifest {
        Manifest::from_json(
            r#"{
                "schema": "connector-manifest/v1",
                "identity": { "family": "testgen5", "display_name": "T", "models": ["T"] },
                "gatt": { "service": "s", "command": "c", "notify": ["n"] },
                "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
                "packets": { "40": "realtime_data" },
                "layouts": {
                    "realtime_data": {
                        "time": { "seconds_offset": 10, "subseconds_offset": 14 },
                        "fields": [
                            { "name": "hr", "stream": "heart_rate", "type": "u8", "offset": 16 }
                        ],
                        "repeats": [
                            {
                                "name": "rr", "stream": "rr_interval", "type": "u16_le",
                                "count_offset": 17, "start_offset": 18, "stride": 2,
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

    /// One gen5 REALTIME_DATA frame: unix `ts`, one HR, no RR.
    fn realtime_frame(ts: u32, hr: u8) -> Vec<u8> {
        let mut payload = vec![0u8; 18];
        payload[0] = 40;
        payload[1] = 1;
        payload[10..14].copy_from_slice(&ts.to_le_bytes());
        payload[16] = hr;
        payload[17] = 0;
        build_frame(WireFormat::Gen5, &payload).unwrap()
    }

    fn capture(hrs: &[(u32, u8)]) -> Capture {
        let mut chunks = Vec::new();
        for &(ts, hr) in hrs {
            chunks.push(realtime_frame(ts, hr));
        }
        Capture {
            device: DeviceId::new(1),
            capture_wall: WallTime::from_unix_seconds(1_752_600_500),
            chunks,
        }
    }

    struct NullTap;
    impl Tap for NullTap {
        fn on_stage(&self, _stage: Stage, _event: TapEvent) {}
    }

    #[test]
    fn realtime_capture_produces_the_expected_snapshot() {
        let store = Store::open_in_memory().unwrap();
        let capture = capture(&[
            (1_752_600_000, 58),
            (1_752_600_001, 61),
            (1_752_600_002, 63),
        ]);
        let snapshot = run_realtime(&realtime_manifest(), &capture, &store, &NullTap).unwrap();

        assert_eq!(
            snapshot.current_bpm,
            Some(63),
            "current is the latest by device time"
        );
        assert_eq!(snapshot.in_range_samples, 3);
        assert_eq!(snapshot.excluded_samples, 0);
        // mean of 58, 61, 63 = 60.666..., rounded to milli-bpm.
        assert_eq!(snapshot.mean_milli_bpm, Some(60_667));
        assert_eq!(snapshot.provenance_id, HR_PROVENANCE.get());
    }

    #[test]
    fn the_same_capture_hashes_identically() {
        let manifest = realtime_manifest();
        let capture = capture(&[(1_752_600_000, 58), (1_752_600_001, 61)]);

        let store_a = Store::open_in_memory().unwrap();
        let a = run_realtime(&manifest, &capture, &store_a, &NullTap).unwrap();
        let store_b = Store::open_in_memory().unwrap();
        let b = run_realtime(&manifest, &capture, &store_b, &NullTap).unwrap();

        assert_eq!(a, b);
        assert_eq!(a.canonical_hash().unwrap(), b.canonical_hash().unwrap());
    }

    #[test]
    fn out_of_range_hr_is_excluded_but_counted() {
        let store = Store::open_in_memory().unwrap();
        // 250 bpm is outside the SQI plausibility band and must not become the current HR.
        let capture = capture(&[(1_752_600_000, 60), (1_752_600_001, 250)]);
        let snapshot = run_realtime(&realtime_manifest(), &capture, &store, &NullTap).unwrap();

        assert_eq!(snapshot.current_bpm, Some(60));
        assert_eq!(snapshot.in_range_samples, 1);
        assert_eq!(snapshot.excluded_samples, 1);
    }

    #[test]
    fn provenance_is_written_and_walks_back_to_the_source() {
        let store = Store::open_in_memory().unwrap();
        let capture = capture(&[(1_752_600_000, 60)]);
        let snapshot = run_realtime(&realtime_manifest(), &capture, &store, &NullTap).unwrap();

        let provenance = store
            .provenance(MetadataId::new(snapshot.provenance_id))
            .unwrap()
            .unwrap();
        assert_eq!(provenance.source_stream, StreamKind::HeartRate);
        assert_eq!(provenance.algorithm_id, "hr_summary");
        assert_eq!(provenance.sample_count, 1);
    }

    #[test]
    fn a_corrupt_frame_is_skipped_and_the_run_completes() {
        let store = Store::open_in_memory().unwrap();
        let mut good = realtime_frame(1_752_600_000, 62);
        let mut corrupt = realtime_frame(1_752_600_001, 61);
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xFF; // break the payload CRC-32
        good.extend(corrupt);

        let capture = Capture {
            device: DeviceId::new(1),
            capture_wall: WallTime::from_unix_seconds(1_752_600_500),
            chunks: vec![good],
        };
        let snapshot = run_realtime(&realtime_manifest(), &capture, &store, &NullTap).unwrap();
        assert_eq!(snapshot.current_bpm, Some(62), "the good frame still lands");
        assert_eq!(snapshot.in_range_samples, 1);
    }
}
