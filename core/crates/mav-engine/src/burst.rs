//! One historical START…END burst, from decoded samples to one atomic commit. The collector owns
//! no radio and no cursor: it scores, places, and persists inside a single transaction, and its
//! receipt is the only thing allowed to become a `BurstPersisted` event — the safe-ack invariant
//! (docs/plans/completed/M5.md) is enforced by construction because a failed transaction returns an
//! error and leaves zero rows.

use crate::recompute::{AffectedDays, LocalDay, Timezone};
use mav_model::error::Result;
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::{RawSample, RawSampleBatch};
use mav_model::time::WallTime;
use mav_obs::stage::Stage;
use mav_obs::tap::{Ids, Tap, TapEvent};
use mav_store::{InsertOutcome, Store};
use mav_timeline::{place_on_wall, Timeline};

/// Proof of a durable commit: counts observed after the transaction succeeded, and the local
/// calendar days the newly inserted samples landed on. Only a value of this type may drive
/// `HistoricalEvent::BurstPersisted`, and only inserted samples dirty a day.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BurstReceipt {
    pub inserted: u32,
    pub duplicates: u32,
    pub affected_days: AffectedDays,
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
    /// back to when a record's own timestamp is implausible, and `timezone` is the injected offset
    /// table that decides which local calendar day each inserted sample dirties.
    pub fn persist(
        self,
        capture_wall: WallTime,
        timezone: &Timezone,
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
                    InsertOutcome::Inserted => {
                        receipt.inserted += 1;
                        let wall = sample.wall_time.unwrap_or(capture_wall);
                        receipt.affected_days.insert(LocalDay::of(wall, timezone));
                    }
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
    use crate::recompute::{OffsetSpan, Timezone};
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
            .persist(WALL, &tz_utc(), &store, &SilentTap)
            .unwrap();
        assert_eq!(receipt.inserted, 2);
        assert_eq!(receipt.duplicates, 0);
        // Epoch-adjacent device times are implausible, so both fall back to the capture wall day.
        assert_eq!(receipt.affected_days.iso(), vec!["2025-07-15"]);
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
            .persist(WALL, &tz_utc(), &store, &SilentTap)
            .unwrap();
        let replay = burst_with_two_records()
            .persist(WALL, &tz_utc(), &store, &SilentTap)
            .unwrap();
        assert_eq!(replay.inserted, 0);
        assert_eq!(replay.duplicates, 2);
        assert!(replay.affected_days.is_empty());
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
        let receipt = burst.persist(WALL, &tz_utc(), &store, &SilentTap).unwrap();
        assert_eq!(receipt.inserted, 2);
        assert_eq!(
            store
                .count_samples(DeviceId::new(1), StreamKind::RrInterval)
                .unwrap(),
            2
        );
    }

    // M5-P6: newly inserted samples dirty their local calendar day; duplicates dirty nothing.

    fn tz_utc() -> Timezone {
        Timezone::fixed("UTC", 0)
    }

    fn hr_at_seconds(unix_seconds: i64, value: u16) -> RawSample {
        raw(
            StreamKind::HeartRate,
            unix_seconds * 1_000_000_000,
            0,
            value,
        )
    }

    #[test]
    fn a_burst_spanning_local_midnight_dirties_two_days() {
        let store = Store::open_in_memory().unwrap();
        let mut burst = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        burst.push(vec![
            hr_at_seconds(1_752_623_999, 60),
            hr_at_seconds(1_752_624_001, 61),
        ]);
        let receipt = burst
            .persist(
                WallTime::from_unix_seconds(1_752_624_100),
                &tz_utc(),
                &store,
                &SilentTap,
            )
            .unwrap();
        assert_eq!(receipt.inserted, 2);
        assert_eq!(
            receipt.affected_days.iso(),
            vec!["2025-07-15", "2025-07-16"]
        );
    }

    #[test]
    fn a_duplicate_only_replay_dirties_no_days() {
        let store = Store::open_in_memory().unwrap();
        let burst = || {
            let mut burst = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
            burst.push(vec![
                hr_at_seconds(1_752_623_999, 60),
                hr_at_seconds(1_752_624_001, 61),
            ]);
            burst
        };
        let wall = WallTime::from_unix_seconds(1_752_624_100);
        let first = burst()
            .persist(wall, &tz_utc(), &store, &SilentTap)
            .unwrap();
        assert_eq!(first.affected_days.len(), 2);
        let replay = burst()
            .persist(wall, &tz_utc(), &store, &SilentTap)
            .unwrap();
        assert_eq!(replay.inserted, 0);
        assert_eq!(replay.duplicates, 2);
        assert!(replay.affected_days.is_empty());
    }

    #[test]
    fn mixed_inserted_and_duplicate_data_dirties_only_inserted_days() {
        let store = Store::open_in_memory().unwrap();
        let wall = WallTime::from_unix_seconds(1_752_624_100);
        let mut first = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        first.push(vec![hr_at_seconds(1_752_600_000, 70)]);
        first.persist(wall, &tz_utc(), &store, &SilentTap).unwrap();

        let mut second = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        second.push(vec![
            hr_at_seconds(1_752_600_000, 70),
            hr_at_seconds(1_752_624_001, 72),
        ]);
        let receipt = second.persist(wall, &tz_utc(), &store, &SilentTap).unwrap();
        assert_eq!(receipt.inserted, 1);
        assert_eq!(receipt.duplicates, 1);
        assert_eq!(receipt.affected_days.iso(), vec!["2025-07-16"]);
    }

    #[test]
    fn the_injected_timezone_moves_the_day_boundary() {
        let samples = || {
            vec![
                hr_at_seconds(1_752_623_999, 60),
                hr_at_seconds(1_752_624_001, 61),
            ]
        };
        let wall = WallTime::from_unix_seconds(1_752_624_100);

        let store = Store::open_in_memory().unwrap();
        let mut burst = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        burst.push(samples());
        let utc = burst.persist(wall, &tz_utc(), &store, &SilentTap).unwrap();
        assert_eq!(utc.affected_days.iso(), vec!["2025-07-15", "2025-07-16"]);

        // Same instants under London's 2025 table: both sit past local midnight in BST.
        let london = Timezone::new(
            "Europe/London",
            vec![
                OffsetSpan {
                    start_unix_seconds: 0,
                    offset_seconds: 0,
                },
                OffsetSpan {
                    start_unix_seconds: 1_743_296_400,
                    offset_seconds: 3_600,
                },
            ],
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        let mut burst = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        burst.push(samples());
        let shifted = burst.persist(wall, &london, &store, &SilentTap).unwrap();
        assert_eq!(shifted.affected_days.iso(), vec!["2025-07-16"]);
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
