//! One historical START…END burst, from decoded samples to one atomic commit. The collector owns
//! no radio and no cursor: it scores, places, and persists inside a single transaction, and its
//! receipt is the only thing allowed to become a `BurstPersisted` event — the safe-ack invariant
//! (docs/plans/active/M5.md) is enforced by construction because a failed transaction returns an
//! error and leaves zero rows.

use mav_model::error::Result;
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::{RawSample, RawSampleBatch};
use mav_model::time::WallTime;
use mav_obs::stage::Stage;
use mav_obs::tap::{Ids, Tap, TapEvent};
use mav_store::{InsertOutcome, Store};
use mav_timeline::{place_on_wall, Timeline};

/// Proof of a durable commit: counts observed after the transaction succeeded. Only a value of
/// this type may drive `HistoricalEvent::BurstPersisted`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BurstReceipt {
    pub inserted: u32,
    pub duplicates: u32,
}

/// Collects the decoded samples between `HISTORY_START` and `HISTORY_END`, then persists them
/// atomically. Dropping a collector without persisting stores nothing and acks nothing.
pub struct HistoricalBurst {
    device: DeviceId,
    sqi_provenance: MetadataId,
    samples: Vec<RawSample>,
}

impl HistoricalBurst {
    pub fn begin(device: DeviceId, sqi_provenance: MetadataId) -> Self {
        Self {
            device,
            sqi_provenance,
            samples: Vec::new(),
        }
    }

    /// Add one frame's decoded samples to the open burst.
    pub fn push(&mut self, decoded: Vec<RawSample>) {
        self.samples.extend(decoded);
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Score, place, and commit the whole burst in one transaction. `Ok` is the durable receipt
    /// (feed `BurstPersisted`); `Err` means the transaction rolled back and zero burst rows exist
    /// (feed `PersistFailed`). `capture_wall` is the phone-side receive time the timeline falls
    /// back to when a record's own timestamp is implausible.
    pub fn persist(
        self,
        capture_wall: WallTime,
        store: &Store,
        tap: &dyn Tap,
    ) -> Result<BurstReceipt> {
        let ids = Ids {
            device: Some(self.device),
            ..Ids::default()
        };
        let batch = RawSampleBatch {
            device: self.device,
            samples: self.samples,
        };
        let scored = mav_sqi::score_batch(&batch, self.sqi_provenance);
        tap.on_stage(
            Stage::Sqi,
            TapEvent::Produced {
                count: scored.len(),
                ids,
                summary: None,
            },
        );

        let mut timeline = Timeline::new();
        let mut receipt = BurstReceipt::default();
        for mut sample in scored {
            place_on_wall(&mut sample, capture_wall);
            if timeline.insert(sample) == mav_timeline::InsertOutcome::Duplicate {
                receipt.duplicates += 1;
            }
        }

        let receipt = store.in_transaction(|txn| {
            let mut receipt = receipt;
            for sample in timeline.drain_ordered() {
                match txn.insert_sample(batch.device, &sample)? {
                    InsertOutcome::Inserted => receipt.inserted += 1,
                    InsertOutcome::Duplicate => receipt.duplicates += 1,
                }
            }
            Ok(receipt)
        })?;

        if receipt.inserted > 0 {
            tap.on_stage(
                Stage::Store,
                TapEvent::Produced {
                    count: receipt.inserted as usize,
                    ids,
                    summary: None,
                },
            );
        }
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::historical::{
        CommandTemplate, HistoricalConfig, HistoricalController, HistoricalEvent, HistoricalState,
        ResponseResult,
    };
    use mav_model::error::{codes, MavError};
    use mav_model::stream::StreamKind;
    use mav_model::time::DeviceTime;
    use mav_store::Store;

    struct SilentTap;

    impl Tap for SilentTap {
        fn on_stage(&self, _stage: Stage, _event: TapEvent) {}
    }

    const WALL: WallTime = WallTime::from_unix_seconds(1_752_600_000);

    fn raw(kind: StreamKind, device_ns: i64, seq: u16, value: u16) -> RawSample {
        RawSample {
            kind,
            device_time: DeviceTime::from_nanos(device_ns),
            seq,
            value: mav_model::raw::RawValue::U16(value),
        }
    }

    fn burst_with_two_records() -> HistoricalBurst {
        let mut burst = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        burst.push(vec![raw(StreamKind::HeartRate, 1_000_000_000, 0, 62)]);
        burst.push(vec![raw(StreamKind::HeartRate, 2_000_000_000, 0, 64)]);
        burst
    }

    #[test]
    fn a_two_record_burst_commits_atomically() {
        let store = Store::open_in_memory().unwrap();
        let receipt = burst_with_two_records()
            .persist(WALL, &store, &SilentTap)
            .unwrap();
        assert_eq!(
            receipt,
            BurstReceipt {
                inserted: 2,
                duplicates: 0
            }
        );
        assert_eq!(
            store
                .count_samples(DeviceId::new(1), StreamKind::HeartRate)
                .unwrap(),
            2
        );
    }

    #[test]
    fn replaying_a_committed_burst_reports_duplicates_and_changes_nothing() {
        let store = Store::open_in_memory().unwrap();
        burst_with_two_records()
            .persist(WALL, &store, &SilentTap)
            .unwrap();
        let replay = burst_with_two_records()
            .persist(WALL, &store, &SilentTap)
            .unwrap();
        assert_eq!(
            replay,
            BurstReceipt {
                inserted: 0,
                duplicates: 2
            }
        );
        assert_eq!(
            store
                .count_samples(DeviceId::new(1), StreamKind::HeartRate)
                .unwrap(),
            2
        );
    }

    #[test]
    fn equal_rr_intervals_with_different_seq_both_survive() {
        let store = Store::open_in_memory().unwrap();
        let mut burst = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        burst.push(vec![
            raw(StreamKind::RrInterval, 1_000_000_000, 0, 800),
            raw(StreamKind::RrInterval, 1_000_000_000, 1, 800),
        ]);
        let receipt = burst.persist(WALL, &store, &SilentTap).unwrap();
        assert_eq!(receipt.inserted, 2);
        assert_eq!(
            store
                .count_samples(DeviceId::new(1), StreamKind::RrInterval)
                .unwrap(),
            2
        );
    }

    /// The cross-layer safe-ack proof: a mid-burst storage failure rolls the transaction back to
    /// zero rows, and feeding the resulting `PersistFailed` to the controller yields no
    /// acknowledgement command and a failed controller.
    #[test]
    fn a_rolled_back_burst_never_reaches_an_ack() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(1);

        // Same shape the burst uses — one transaction — with a failure injected on record two.
        let first = || raw(StreamKind::HeartRate, 1_000_000_000, 0, 62);
        let error = store
            .in_transaction(|txn| -> Result<BurstReceipt> {
                let batch = RawSampleBatch {
                    device,
                    samples: vec![first()],
                };
                for sample in mav_sqi::score_batch(&batch, MetadataId::new(1)) {
                    txn.insert_sample(device, &sample)?;
                }
                Err(MavError::new(
                    codes::STORAGE_QUERY,
                    "injected failure on record two",
                ))
            })
            .unwrap_err();
        assert_eq!(
            store.count_samples(device, StreamKind::HeartRate).unwrap(),
            0
        );

        let template = |opcode: u8| CommandTemplate {
            opcode,
            b3: None,
            payload: vec![0x00],
        };
        let mut controller = HistoricalController::new(HistoricalConfig {
            get_data_range: template(34),
            send_historical: template(22),
            acknowledge: template(23),
            max_retries: 1,
            max_ack_payload_bytes: 64,
        });
        let start = controller.step(HistoricalEvent::Start, &SilentTap).unwrap();
        let range_seq = start.commands[0].seq;
        controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: 34,
                    origin_seq: range_seq,
                    result: ResponseResult::Ok,
                },
                &SilentTap,
            )
            .unwrap();
        controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: 22,
                    origin_seq: range_seq + 1,
                    result: ResponseResult::Ok,
                },
                &SilentTap,
            )
            .unwrap();
        controller
            .step(HistoricalEvent::BurstStarted, &SilentTap)
            .unwrap();
        controller
            .step(
                HistoricalEvent::BurstEnded {
                    ack_payload: vec![0xDE, 0xAD],
                    record_count: 2,
                },
                &SilentTap,
            )
            .unwrap();
        let stepped = controller
            .step(HistoricalEvent::PersistFailed { error }, &SilentTap)
            .unwrap_err();
        assert_eq!(stepped.code, codes::STORAGE_QUERY);
        assert_eq!(controller.state(), HistoricalState::Failed);
    }
}
