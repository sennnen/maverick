//! Forward-only numbered migrations. Each entry moves the schema from version N-1 to N;
//! `MIGRATIONS[0]` builds version 1 from an empty database. There are no down-migrations
//! (docs/storage.md): a wrong migration is fixed by the next one, never by rolling back.
//!
//! Most steps are a string of SQL. A step that has to re-encode stored values is a Rust function
//! instead, because reshaping a row means decoding it, and SQLite cannot decode our value types.

use mav_model::raw::RawValue;
use mav_model::stream::{Placement, RejectReason, StreamKind};
use mav_model::time::WallTime;
use rusqlite::{params, Connection};

/// The schema version this build understands. Opening a database recorded as newer than this is
/// refused rather than migrated.
pub const CURRENT_SCHEMA_VERSION: i64 = 5;

pub enum Migration {
    Sql(&'static str),
    Rust(fn(&Connection) -> rusqlite::Result<()>),
}

pub const MIGRATIONS: &[Migration] = &[
    // v1 — the Milestone 1 slice.
    Migration::Sql(
        "
    CREATE TABLE sample (
        device_id      INTEGER NOT NULL,
        stream         TEXT    NOT NULL,
        device_time_ns INTEGER NOT NULL,
        seq            INTEGER NOT NULL,
        value_json     TEXT    NOT NULL,
        wall_time_ns   INTEGER,
        quality_score  REAL    NOT NULL,
        quality_reason TEXT,
        provenance_id  INTEGER NOT NULL,
        PRIMARY KEY (device_id, stream, device_time_ns, seq, value_json)
    ) WITHOUT ROWID;

    CREATE TABLE provenance (
        metadata_id       INTEGER PRIMARY KEY,
        source_stream     TEXT    NOT NULL,
        quality           REAL    NOT NULL,
        algorithm_id      TEXT    NOT NULL,
        algorithm_version TEXT    NOT NULL,
        sample_count      INTEGER NOT NULL
    );

    CREATE TABLE error_journal (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        code         INTEGER NOT NULL,
        category     TEXT    NOT NULL,
        severity     TEXT    NOT NULL,
        message      TEXT    NOT NULL,
        context_json TEXT    NOT NULL,
        created_ns   INTEGER NOT NULL
    );
    ",
    ),
    // v2 — the derived daily snapshot. Derived by construction: dropping every row here and
    // recomputing from `sample` must reproduce it byte for byte, which is what makes it safe to
    // rebuild after an algorithm change rather than migrate.
    Migration::Sql(
        "
    CREATE TABLE daily_snapshot (
        device_id     INTEGER NOT NULL,
        local_day     INTEGER NOT NULL,
        snapshot_json TEXT    NOT NULL,
        algorithms    TEXT    NOT NULL,
        computed_ns   INTEGER NOT NULL,
        PRIMARY KEY (device_id, local_day)
    ) WITHOUT ROWID;
    ",
    ),
    // v3 — an all-integer sample table with a wall-time index. The v1 key held the value as JSON
    // text inside a WITHOUT ROWID primary key, so every row carried its own string and every
    // lookup compared strings; and there was no index at all, so reading one day meant scanning
    // the whole stream. Placement also moves out of the quality reason, which is why the reason
    // column loses `implausible_timestamp`.
    Migration::Rust(rebuild_sample_as_integers),
    // v4 — the nightly variability memo. A longitudinal reading looks back sixty nights, and
    // re-deriving all sixty from raw beats on every render is what made opening the app read the
    // whole database. One row per device, day and interval stream; a NULL value means the night
    // was computed and held nothing usable, which is different from having no row at all.
    Migration::Sql(
        "
    CREATE TABLE nightly_variability (
        device_id INTEGER NOT NULL,
        local_day INTEGER NOT NULL,
        stream    INTEGER NOT NULL,
        rmssd_ms  REAL,
        PRIMARY KEY (device_id, stream, local_day)
    ) WITHOUT ROWID;
    ",
    ),
    // v5 — repair the databases the first cut of v3 mislabelled. It carried `rr_interval` across
    // literally, so beats an optical strap timed came out as electrical ones and would have been
    // published as heart-rate variability. Nothing had streamed an electrical interval by the time
    // this landed — the only connector that can, Generic HR Monitor, ships alongside it — so every
    // interval already stored is optical and the remap is total. The derived rows go with them:
    // they were computed under the wrong label.
    Migration::Sql(
        "
    UPDATE sample SET stream = 20 WHERE stream = 1;
    DELETE FROM nightly_variability;
    DELETE FROM daily_snapshot;
    ",
    ),
];

/// Widen every stored sample from the v1 text encoding to the v3 integer one.
///
/// Two v1 facts cannot be carried across as they stand.
///
/// The old `implausible_timestamp` reject reason conflated a clock the timeline corrected with one
/// it could not, and the distinction was never recorded. Those rows become `CaptureFallback` — the
/// weaker claim, which does not assert that the gaps between samples survived.
///
/// And v1's `rr_interval` was the only interval stream, so every producer of it was an optical
/// strap (ADR-027). Read literally, those rows would migrate into `RrInterval`, which now means an
/// electrical R peak and licenses the `heart_rate_variability` label — the one claim this project
/// exists to keep honest. They migrate to `PulseInterval` instead.
/// Every interval stored before the split came from an optical pulse, whatever it was called.
fn optical_before_the_split(stream: StreamKind) -> StreamKind {
    match stream {
        StreamKind::RrInterval => StreamKind::PulseInterval,
        other => other,
    }
}

fn rebuild_sample_as_integers(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
    CREATE TABLE sample_v3 (
        device_id      INTEGER NOT NULL,
        stream         INTEGER NOT NULL,
        device_time_ns INTEGER NOT NULL,
        seq            INTEGER NOT NULL,
        value_tag      INTEGER NOT NULL,
        value_bits     INTEGER NOT NULL,
        wall_time_ns   INTEGER,
        placement      INTEGER NOT NULL,
        quality_score  REAL    NOT NULL,
        quality_reason INTEGER,
        provenance_id  INTEGER NOT NULL,
        PRIMARY KEY (device_id, stream, device_time_ns, seq, value_tag, value_bits)
    ) WITHOUT ROWID;
    ",
    )?;

    let mut read = conn.prepare(
        "SELECT device_id, stream, device_time_ns, seq, value_json, wall_time_ns, \
                quality_score, quality_reason, provenance_id FROM sample",
    )?;
    let rows = read
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut write = conn.prepare(
        "INSERT OR IGNORE INTO sample_v3 \
         (device_id, stream, device_time_ns, seq, value_tag, value_bits, wall_time_ns, \
          placement, quality_score, quality_reason, provenance_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for (
        device,
        stream_json,
        device_ns,
        seq,
        value_json,
        wall_ns,
        score,
        reason_json,
        provenance,
    ) in rows
    {
        let (Ok(stream), Ok(value)) = (
            serde_json::from_str::<StreamKind>(&stream_json),
            serde_json::from_str::<RawValue>(&value_json),
        ) else {
            continue;
        };
        let stream = optical_before_the_split(stream);
        let clock_was_untrusted = reason_json.as_deref() == Some("\"implausible_timestamp\"");
        let reason = reason_json
            .filter(|_| !clock_was_untrusted)
            .and_then(|text| serde_json::from_str::<RejectReason>(&text).ok());
        let placement = match (wall_ns.map(WallTime::from_nanos), clock_was_untrusted) {
            (None, _) => Placement::Unplaced,
            (Some(at), true) => Placement::CaptureFallback(at),
            (Some(at), false) => Placement::DeviceClock(at),
        };
        write.execute(params![
            device,
            stream.code(),
            device_ns,
            seq,
            value.tag(),
            value.key_bits() as i64,
            wall_ns,
            placement.code(),
            score,
            reason.map(|r| r.code()),
            provenance,
        ])?;
    }
    drop(write);
    drop(read);

    conn.execute_batch(
        "
    DROP TABLE sample;
    ALTER TABLE sample_v3 RENAME TO sample;
    CREATE INDEX sample_by_wall ON sample (device_id, stream, wall_time_ns);
    ",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_count_matches_current_version() {
        assert_eq!(MIGRATIONS.len() as i64, CURRENT_SCHEMA_VERSION);
    }
}
