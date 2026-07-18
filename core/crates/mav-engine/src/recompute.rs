//! The affected-day recompute trigger (M5-P6): which local calendar days a completed historical
//! sync dirtied, and the cache-invalidation hook that recomputes only those windows.
//!
//! Timezone data is injected as an explicit offset table — the host owns the tzdb, the core owns
//! the arithmetic. No code here reads the system timezone or clock, so every day boundary is
//! reproducible from the inputs alone, and a tzdb update on the phone can never silently move a
//! frozen fixture hash.

use mav_model::error::{codes, MavError, Result};
use mav_model::time::WallTime;
use mav_model::version::Version;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

const SECONDS_PER_DAY: i64 = 86_400;

/// One UTC-offset regime: from `start_unix_seconds` (inclusive) until the next span begins, local
/// time is UTC plus `offset_seconds`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OffsetSpan {
    pub start_unix_seconds: i64,
    pub offset_seconds: i32,
}

/// An injected timezone: an IANA id for display and provenance, and the explicit offset table the
/// host derived from its own tzdb. Instants before the first span use the first span's offset.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Timezone {
    id: String,
    spans: Vec<OffsetSpan>,
}

impl Timezone {
    pub fn new(id: impl Into<String>, spans: Vec<OffsetSpan>) -> Result<Self> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(invalid_timezone("timezone id must not be blank"));
        }
        if spans.is_empty() {
            return Err(invalid_timezone("timezone offset table must not be empty"));
        }
        if spans
            .windows(2)
            .any(|pair| pair[0].start_unix_seconds >= pair[1].start_unix_seconds)
        {
            return Err(invalid_timezone(
                "timezone offset spans must be strictly ascending",
            ));
        }
        Ok(Self { id, spans })
    }

    /// A single-offset timezone, for UTC and for tests; a fixed offset needs no table.
    pub fn fixed(id: &str, offset_seconds: i32) -> Self {
        Self {
            id: id.to_owned(),
            spans: vec![OffsetSpan {
                start_unix_seconds: i64::MIN,
                offset_seconds,
            }],
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn offset_at(&self, at: WallTime) -> i32 {
        let seconds = at.as_unix_seconds();
        let position = self
            .spans
            .partition_point(|span| span.start_unix_seconds <= seconds);
        self.spans[position.saturating_sub(1)].offset_seconds
    }
}

fn invalid_timezone(message: &'static str) -> MavError {
    MavError::new(codes::FFI_RUNTIME_STATE, message)
}

/// A local calendar day, stored as whole days since 1970-01-01 in the injected timezone. It
/// renders as an ISO date, which is also its canonical JSON form.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LocalDay(i64);

impl LocalDay {
    pub fn of(at: WallTime, timezone: &Timezone) -> Self {
        let local_seconds = at.as_unix_seconds() + i64::from(timezone.offset_at(at));
        Self(local_seconds.div_euclid(SECONDS_PER_DAY))
    }

    pub const fn from_index(index: i64) -> Self {
        Self(index)
    }

    pub const fn index(self) -> i64 {
        self.0
    }
}

impl fmt::Display for LocalDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = civil_from_days(self.0);
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

impl Serialize for LocalDay {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Howard Hinnant's days-to-civil algorithm: pure integer math, valid across the whole i64 range
/// this project can reach.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The sorted, unique set of local days a sync inserted new samples into. Duplicates never enter
/// it, so an all-duplicate replay reports an empty set.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize)]
pub struct AffectedDays(Vec<LocalDay>);

impl AffectedDays {
    pub fn insert(&mut self, day: LocalDay) {
        if let Err(position) = self.0.binary_search(&day) {
            self.0.insert(position, day);
        }
    }

    pub fn union(&mut self, other: &AffectedDays) {
        for day in &other.0 {
            self.insert(*day);
        }
    }

    pub fn contains(&self, day: LocalDay) -> bool {
        self.0.binary_search(&day).is_ok()
    }

    pub fn days(&self) -> &[LocalDay] {
        &self.0
    }

    pub fn iso(&self) -> Vec<String> {
        self.0.iter().map(LocalDay::to_string).collect()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The completion trigger: a finished sync names exactly the days it dirtied. It exists only for
/// syncs that reached `HistoricalState::Complete`; an interrupted sync must not trigger anything.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecomputeTrigger {
    pub days: AffectedDays,
}

/// Accumulates affected days across the bursts of one historical sync. Feed it every burst
/// receipt, then ask for the completion trigger with the controller's final state.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SyncDays {
    days: AffectedDays,
}

impl SyncDays {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn absorb(&mut self, receipt: &crate::burst::BurstReceipt) {
        self.days.union(&receipt.affected_days);
    }

    pub fn completion_trigger(self, state: crate::HistoricalState) -> Option<RecomputeTrigger> {
        (state == crate::HistoricalState::Complete).then_some(RecomputeTrigger { days: self.days })
    }
}

/// The identity of one cached computation: the metric, the algorithm version that produced it,
/// and the inclusive local-day window it read. docs/architecture.md pins this key shape.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct CacheKey {
    pub metric: String,
    pub algorithm_version: Version,
    pub first_day: LocalDay,
    pub last_day: LocalDay,
}

impl CacheKey {
    fn intersects(&self, days: &AffectedDays) -> bool {
        days.days()
            .iter()
            .any(|day| (self.first_day..=self.last_day).contains(day))
    }
}

/// The recompute cache hook: dirtied days evict exactly the entries whose window they intersect,
/// and the evicted keys are what the engine recomputes and re-inserts.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RecomputeCache<V> {
    entries: BTreeMap<CacheKey, V>,
}

impl<V> RecomputeCache<V> {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn put(&mut self, key: CacheKey, value: V) {
        self.entries.insert(key, value);
    }

    pub fn get(&self, key: &CacheKey) -> Option<&V> {
        self.entries.get(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove every entry whose input window intersects the dirtied days and return their keys,
    /// sorted. Entries outside the window are untouched: an empty day set evicts nothing.
    pub fn invalidate(&mut self, days: &AffectedDays) -> Vec<CacheKey> {
        let evicted: Vec<CacheKey> = self
            .entries
            .keys()
            .filter(|key| key.intersects(days))
            .cloned()
            .collect();
        for key in &evicted {
            self.entries.remove(key);
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::burst::HistoricalBurst;
    use crate::historical::{
        CommandTemplate, HistoricalConfig, HistoricalController, HistoricalEvent, ResponseResult,
    };
    use crate::{run_realtime_output, Manifest, Store};
    use mav_model::error::codes;
    use mav_model::ids::{DeviceId, MetadataId};
    use mav_model::raw::RawSample;
    use mav_model::stream::StreamKind;
    use mav_model::time::{DeviceTime, WallTime};
    use mav_model::version::Version;
    use mav_obs::stage::Stage;
    use mav_obs::tap::{Tap, TapEvent};
    use std::path::PathBuf;

    struct SilentTap;

    impl Tap for SilentTap {
        fn on_stage(&self, _stage: Stage, _event: TapEvent) {}
    }

    fn utc() -> Timezone {
        Timezone::fixed("UTC", 0)
    }

    fn day(index: i64) -> LocalDay {
        LocalDay::from_index(index)
    }

    #[test]
    fn local_days_render_civil_dates_exactly() {
        assert_eq!(day(0).to_string(), "1970-01-01");
        assert_eq!(day(-1).to_string(), "1969-12-31");
        assert_eq!(day(20_285).to_string(), "2025-07-16");
        assert_eq!(day(19_190).to_string(), "2022-07-17");
    }

    #[test]
    fn offset_lookup_uses_the_last_span_at_or_before_the_instant() {
        // London 2025: GMT until the BST switch at 2025-03-30T01:00:00Z, then +1h.
        let tz = Timezone::new(
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
        assert_eq!(tz.offset_at(WallTime::from_unix_seconds(1_743_296_399)), 0);
        assert_eq!(
            tz.offset_at(WallTime::from_unix_seconds(1_743_296_400)),
            3_600
        );
        assert_eq!(
            tz.offset_at(WallTime::from_unix_seconds(1_752_624_000)),
            3_600
        );
    }

    #[test]
    fn an_invalid_timezone_table_is_rejected() {
        let span = |start| OffsetSpan {
            start_unix_seconds: start,
            offset_seconds: 0,
        };
        let empty = Timezone::new("UTC", Vec::new()).unwrap_err();
        assert_eq!(empty.code, codes::FFI_RUNTIME_STATE);
        let unsorted = Timezone::new("X", vec![span(100), span(50)]).unwrap_err();
        assert_eq!(unsorted.code, codes::FFI_RUNTIME_STATE);
        let blank_id = Timezone::new("  ", vec![span(0)]).unwrap_err();
        assert_eq!(blank_id.code, codes::FFI_RUNTIME_STATE);
    }

    #[test]
    fn affected_days_stay_sorted_and_unique() {
        let mut days = AffectedDays::default();
        days.insert(day(20_285));
        days.insert(day(20_284));
        days.insert(day(20_285));
        assert_eq!(days.days(), &[day(20_284), day(20_285)]);
        assert_eq!(days.iso(), vec!["2025-07-15", "2025-07-16"]);

        let mut other = AffectedDays::default();
        other.insert(day(20_290));
        other.insert(day(20_284));
        days.union(&other);
        assert_eq!(days.days(), &[day(20_284), day(20_285), day(20_290)]);
    }

    fn config() -> HistoricalConfig {
        let template = |opcode| CommandTemplate {
            opcode,
            b3: None,
            payload: vec![0x00],
        };
        HistoricalConfig {
            get_data_range: template(34),
            send_historical: template(22),
            acknowledge: template(23),
            max_retries: 1,
            max_ack_payload_bytes: 64,
        }
    }

    fn drive_through_one_burst(controller: &mut HistoricalController) {
        let range = controller
            .step(HistoricalEvent::Start, &SilentTap)
            .unwrap()
            .commands
            .remove(0);
        let send = controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: range.opcode,
                    origin_seq: range.seq,
                    result: ResponseResult::Ok,
                },
                &SilentTap,
            )
            .unwrap()
            .commands
            .remove(0);
        controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: send.opcode,
                    origin_seq: send.seq,
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
                    ack_payload: vec![0x01],
                    record_count: 1,
                },
                &SilentTap,
            )
            .unwrap();
        controller
            .step(HistoricalEvent::BurstPersisted, &SilentTap)
            .unwrap();
    }

    fn persisted_receipt(store: &Store) -> crate::burst::BurstReceipt {
        let mut burst = HistoricalBurst::begin(DeviceId::new(1), MetadataId::new(1));
        burst.push(vec![RawSample {
            kind: StreamKind::HeartRate,
            device_time: DeviceTime::from_nanos(1_752_624_001 * 1_000_000_000),
            seq: 0,
            value: mav_model::raw::RawValue::U16(61),
        }]);
        burst
            .persist(
                WallTime::from_unix_seconds(1_752_624_100),
                &utc(),
                store,
                &SilentTap,
            )
            .unwrap()
    }

    #[test]
    fn a_completed_sync_emits_the_exact_affected_days() {
        let store = Store::open_in_memory().unwrap();
        let mut controller = HistoricalController::new(config());
        let mut sync = SyncDays::new();
        drive_through_one_burst(&mut controller);
        sync.absorb(&persisted_receipt(&store));
        controller
            .step(HistoricalEvent::HistoryComplete, &SilentTap)
            .unwrap();
        let trigger = sync.completion_trigger(controller.state()).unwrap();
        assert_eq!(trigger.days.iso(), vec!["2025-07-16"]);
    }

    #[test]
    fn an_interrupted_sync_emits_no_completion_trigger() {
        let store = Store::open_in_memory().unwrap();
        let mut controller = HistoricalController::new(config());
        let mut sync = SyncDays::new();
        drive_through_one_burst(&mut controller);
        sync.absorb(&persisted_receipt(&store));
        controller
            .step(HistoricalEvent::Disconnect, &SilentTap)
            .unwrap();
        assert!(sync.completion_trigger(controller.state()).is_none());
    }

    fn key(metric: &str, first: i64, last: i64) -> CacheKey {
        CacheKey {
            metric: metric.to_owned(),
            algorithm_version: Version::new(1, 0, 0),
            first_day: day(first),
            last_day: day(last),
        }
    }

    #[test]
    fn invalidation_evicts_only_intersecting_windows() {
        let mut cache = RecomputeCache::new();
        cache.put(key("a", 20_280, 20_283), "before".to_owned());
        cache.put(key("b", 20_282, 20_285), "spanning".to_owned());
        cache.put(key("c", 20_290, 20_290), "after".to_owned());

        let mut dirtied = AffectedDays::default();
        dirtied.insert(day(20_284));
        let evicted = cache.invalidate(&dirtied);
        assert_eq!(evicted, vec![key("b", 20_282, 20_285)]);
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&key("a", 20_280, 20_283)).is_some());
        assert!(cache.get(&key("c", 20_290, 20_290)).is_some());

        let untouched = cache.invalidate(&AffectedDays::default());
        assert!(untouched.is_empty());
        assert_eq!(cache.len(), 2);
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/replay")
            .join(name)
    }

    #[test]
    fn a_recompute_after_invalidation_reproduces_identical_analytics() {
        let manifest = Manifest::from_json(
            &std::fs::read_to_string(fixture("realtime_rr_prv_v2.manifest.json")).unwrap(),
        )
        .unwrap();
        let capture = crate::Capture::from_json(
            &std::fs::read_to_string(fixture("realtime_rr_prv_v2.capture.json")).unwrap(),
        )
        .unwrap();
        let run = |store: &Store| {
            run_realtime_output(&manifest, &capture, store, &SilentTap)
                .unwrap()
                .analytics
                .canonical_hash()
                .unwrap()
        };

        let mut cache = RecomputeCache::new();
        let window = key(mav_analytic::HRV_ALGORITHM, 20_284, 20_284);
        let first = run(&Store::open_in_memory().unwrap());
        cache.put(window.clone(), first.clone());

        let mut dirtied = AffectedDays::default();
        dirtied.insert(day(20_284));
        let evicted = cache.invalidate(&dirtied);
        assert_eq!(evicted, vec![window.clone()]);

        let recomputed = run(&Store::open_in_memory().unwrap());
        assert_eq!(recomputed, first);
        cache.put(window.clone(), recomputed);
        assert_eq!(cache.get(&window), Some(&first));
    }
}
