//! Local calendar days and the injected timezone they are measured in.
//!
//! Timezone data is an explicit offset table — the host owns the tzdb, the core owns the
//! arithmetic. No code here reads the system timezone or clock, so every day boundary is
//! reproducible from the inputs alone, and a tzdb update on the phone can never silently move a
//! frozen fixture hash.

use mav_model::error::{codes, MavError, Result};
use mav_model::time::WallTime;
use serde::Serialize;
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

    pub const fn offset(self, days: i64) -> Self {
        Self(self.0.saturating_add(days))
    }

    /// The instant this local day begins. Resolved twice because the offset in force at local
    /// midnight is what defines the boundary, and a first guess is needed to find it — a second
    /// pass is enough for any real transition, which is at most a few hours wide.
    pub fn start(self, timezone: &Timezone) -> WallTime {
        let local_midnight = self.0.saturating_mul(SECONDS_PER_DAY);
        let mut at = WallTime::from_unix_seconds(local_midnight);
        for _ in 0..2 {
            at = WallTime::from_unix_seconds(local_midnight - i64::from(timezone.offset_at(at)));
        }
        at
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

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::error::codes;
    use mav_model::time::WallTime;

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

    /// The day a boundary instant belongs to has to be the day that claims it, or a read window
    /// and the bucket it fills disagree and samples fall between them.
    #[test]
    fn a_days_span_is_exactly_the_instants_that_belong_to_it() {
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
        for index in [20_180i64, 20_181, 20_285] {
            let today = day(index);
            let start = today.start(&london);
            let next = today.offset(1).start(&london);
            assert_eq!(LocalDay::of(start, &london), today, "{today} start");
            assert_eq!(
                LocalDay::of(WallTime::from_nanos(next.as_nanos() - 1), &london),
                today,
                "{today} end"
            );
            assert_eq!(LocalDay::of(next, &london), today.offset(1));
        }
    }

    /// The spring-forward day is 23 hours long, and the read window has to be too.
    #[test]
    fn a_daylight_saving_transition_shortens_the_day() {
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
        let transition = LocalDay::of(WallTime::from_unix_seconds(1_743_296_400), &london);
        let length = transition.offset(1).start(&london).as_unix_seconds()
            - transition.start(&london).as_unix_seconds();
        assert_eq!(length, 23 * 3_600);
    }
}
