//! The synchronous pipeline for the realtime slice: a capture of raw notification bytes in, a
//! snapshot out, running decode, signal quality, timeline, storage, feature, and snapshot in the
//! fixed order docs/pipeline.md sets. It is the shared path `mav-replay` and the FFI both drive, so
//! that a capture replayed offline runs the identical code the live device would.

use crate::snapshot::{AnalyticsSnapshot, Snapshot};
use mav_analytic::{negotiate, time_domain, IntervalSource, HRV_ALGORITHM, HRV_VERSION};
use mav_codec::codec::{DeviceCodec, ManifestCodec};
use mav_codec::kv::MemoryKv;
use mav_codec::manifest::{IntervalSourceConfig, Manifest};
use mav_feature::hr::{hr_summary, HR_FEATURE_ALGORITHM, HR_FEATURE_VERSION};
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
use serde::Deserialize;

/// Fixed provenance ids for the M1 slice, so the snapshot is byte-for-byte reproducible.
const SQI_PROVENANCE: MetadataId = MetadataId::new(1);
const HR_PROVENANCE: MetadataId = MetadataId::new(2);
const HRV_PROVENANCE: MetadataId = MetadataId::new(3);

const CAPTURE_SCHEMA: &str = "capture/v1";

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

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PipelineOutput {
    pub snapshot: Snapshot,
    pub analytics: AnalyticsSnapshot,
}

#[derive(Deserialize)]
struct CaptureFile {
    schema: String,
    device_id: u64,
    capture_wall_unix: i64,
    chunks_hex: Vec<String>,
}

impl Capture {
    /// Parse a `capture/v1` file: a device id, a capture wall time, and hex notification chunks.
    /// Both `mav-replay` and the FFI parse captures through here, so the two agree by construction.
    pub fn from_json(json: &str) -> Result<Self> {
        let file: CaptureFile = serde_json::from_str(json).map_err(|e| {
            MavError::new(codes::STORAGE_SERIALIZE, "capture does not parse").context(e.to_string())
        })?;
        if file.schema != CAPTURE_SCHEMA {
            return Err(
                MavError::new(codes::STORAGE_SERIALIZE, "unsupported capture schema")
                    .context(format!("got {:?}, want {CAPTURE_SCHEMA:?}", file.schema)),
            );
        }
        let mut chunks = Vec::with_capacity(file.chunks_hex.len());
        for hex in &file.chunks_hex {
            chunks.push(unhex(hex)?);
        }
        Ok(Capture {
            device: DeviceId::new(file.device_id),
            capture_wall: WallTime::from_unix_seconds(file.capture_wall_unix),
            chunks,
        })
    }
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(MavError::new(
            codes::STORAGE_SERIALIZE,
            "hex string has an odd length",
        ));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| {
                MavError::new(codes::STORAGE_SERIALIZE, "invalid hex in capture")
                    .context(e.to_string())
            })
        })
        .collect()
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
    Ok(run_realtime_output(manifest, capture, store, tap)?.snapshot)
}

pub fn run_realtime_output(
    manifest: &Manifest,
    capture: &Capture,
    store: &Store,
    tap: &dyn Tap,
) -> Result<PipelineOutput> {
    let mut reassembler = Reassembler::with_spec(manifest.frame.to_spec()?);
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
    let interval_source = match manifest.interval_source {
        IntervalSourceConfig::Ecg => IntervalSource::Ecg,
        IntervalSourceConfig::Ppg => IntervalSource::Ppg,
        IntervalSourceConfig::Unknown => IntervalSource::Unknown,
    };
    let rr_samples = store.samples(capture.device, StreamKind::RrInterval)?;
    let variability = time_domain(&rr_samples, interval_source, HRV_PROVENANCE);
    if let Some(value) = &variability {
        store.upsert_provenance(&Provenance {
            metadata: HRV_PROVENANCE,
            source_stream: StreamKind::RrInterval,
            quality: 1.0,
            algorithm_id: HRV_ALGORITHM.to_owned(),
            algorithm_version: HRV_VERSION,
            sample_count: value.interval_count,
        })?;
        tap.on_stage(
            Stage::Metrics,
            TapEvent::Produced {
                count: 1,
                ids,
                summary: None,
            },
        );
    }
    let analytics = AnalyticsSnapshot::new(
        interval_source,
        variability.as_ref(),
        negotiate(&manifest.capabilities),
    );
    tap.on_stage(
        Stage::Snapshots,
        TapEvent::Produced {
            count: 2,
            ids,
            summary: None,
        },
    );
    Ok(PipelineOutput {
        snapshot,
        analytics,
    })
}

/// Parse a manifest and a capture from JSON, run them through a fresh in-memory store, and return
/// the snapshot. This is the stateless entry the FFI calls; a replay against a persistent store
/// uses [`run_realtime`] directly.
pub fn run_realtime_json(
    manifest_json: &str,
    capture_json: &str,
    tap: &dyn Tap,
) -> Result<Snapshot> {
    let manifest = Manifest::from_json(manifest_json)?;
    let capture = Capture::from_json(capture_json)?;
    let store = Store::open_in_memory()?;
    run_realtime(&manifest, &capture, &store, tap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_frame::frame::{build_frame, WireFormat};

    fn realtime_manifest() -> Manifest {
        Manifest::from_json(
            r#"{
                "schema": "connector-manifest/v1",
                "identity": { "family": "testgen5", "display_name": "T", "models": ["T"] },
                "gatt": { "service": "s", "command": "c", "notify": ["n"] },
                "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
                "interval_source": "ppg",
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

    fn realtime_frame_with_rr(ts: u32, hr: u8, rr: &[u16]) -> Vec<u8> {
        let mut payload = vec![0u8; 18 + rr.len() * 2];
        payload[0] = 40;
        payload[1] = 1;
        payload[10..14].copy_from_slice(&ts.to_le_bytes());
        payload[16] = hr;
        payload[17] = rr.len() as u8;
        for (index, value) in rr.iter().enumerate() {
            let offset = 18 + index * 2;
            payload[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
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
    fn rr_capture_produces_prv_and_honest_recovery_availability() {
        let capture = Capture {
            device: DeviceId::new(1),
            capture_wall: WallTime::from_unix_seconds(1_752_600_500),
            chunks: vec![realtime_frame_with_rr(
                1_752_600_000,
                72,
                &[800, 800, 850, 790, 900, 0, 50],
            )],
        };
        let store = Store::open_in_memory().unwrap();
        let output = run_realtime_output(&realtime_manifest(), &capture, &store, &NullTap).unwrap();

        assert_eq!(output.analytics.interval_source, "ppg");
        assert_eq!(
            output.analytics.variability_label.as_deref(),
            Some("pulse_rate_variability")
        );
        assert_eq!(output.analytics.mean_interval_micros, Some(828_000));
        assert_eq!(output.analytics.rmssd_micros, Some(67_454));
        assert_eq!(output.analytics.sdnn_micros, Some(46_583));
        assert_eq!(output.analytics.nn50_count, Some(2));
        assert_eq!(output.analytics.pnn50_milli_percent, Some(50_000));
        assert_eq!(output.analytics.interval_count, 5);
        assert_eq!(output.analytics.excluded_interval_count, 1);
        assert!(output
            .analytics
            .availability
            .iter()
            .any(
                |item| item.analytic == mav_analytic::AnalyticId::TimeDomainHrv && item.available
            ));
        assert!(output.analytics.availability.iter().any(|item| {
            item.analytic == mav_analytic::AnalyticId::Recovery
                && item.reason == Some(mav_analytic::UnavailableReason::AlgorithmNotAdmitted)
        }));
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
    fn capture_parses_from_json() {
        let json = r#"{
            "schema": "capture/v1",
            "device_id": 7,
            "capture_wall_unix": 1752600500,
            "chunks_hex": ["aabb", "ccdd"]
        }"#;
        let capture = Capture::from_json(json).unwrap();
        assert_eq!(capture.device, DeviceId::new(7));
        assert_eq!(capture.chunks, vec![vec![0xaa, 0xbb], vec![0xcc, 0xdd]]);
    }

    #[test]
    fn capture_rejects_a_wrong_schema_and_odd_hex() {
        assert!(Capture::from_json(
            r#"{"schema":"capture/v9","device_id":1,"capture_wall_unix":0,"chunks_hex":[]}"#
        )
        .is_err());
        assert!(Capture::from_json(
            r#"{"schema":"capture/v1","device_id":1,"capture_wall_unix":0,"chunks_hex":["abc"]}"#
        )
        .is_err());
    }

    #[test]
    fn run_realtime_json_matches_the_struct_path() {
        let manifest = realtime_manifest();
        let manifest_json = serde_json::to_string(&manifest).unwrap();
        let capture_json = r#"{
            "schema": "capture/v1",
            "device_id": 1,
            "capture_wall_unix": 1752600500,
            "chunks_hex": []
        }"#;
        // An empty capture yields an all-none snapshot, which both paths must agree on.
        let via_json = run_realtime_json(&manifest_json, capture_json, &NullTap).unwrap();
        assert_eq!(via_json.current_bpm, None);
        assert_eq!(via_json.in_range_samples, 0);
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
