use crate::migrations::{Migration, CURRENT_SCHEMA_VERSION, MIGRATIONS};
use mav_model::error::{codes, Category, MavError, Result, Severity};
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::RawValue;
use mav_model::stream::{Placement, Quality, RejectReason, Sample, StreamKind};
use mav_model::time::{DeviceTime, WallTime};
use mav_model::version::Version;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Whether an insert added a row or found the natural key already present. A re-sync produces
/// `Duplicate`, which the caller counts rather than treats as an error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertOutcome {
    Inserted,
    Duplicate,
}

/// A provenance row: what produced a derived value. Keyed by `MetadataId`, read during walk-back.
#[derive(Clone, PartialEq, Debug)]
pub struct Provenance {
    pub metadata: MetadataId,
    pub source_stream: StreamKind,
    pub quality: f32,
    pub algorithm_id: String,
    pub algorithm_version: Version,
    pub sample_count: u32,
}

/// One row of the durable error journal.
#[derive(Clone, PartialEq, Debug)]
pub struct JournalEntry {
    pub id: i64,
    pub code: u16,
    pub category: Category,
    pub severity: Severity,
    pub message: String,
    pub context: Vec<String>,
    pub created_ns: i64,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if absent) the database at `path`, applying any pending migrations.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn =
            Connection::open(path).map_err(|e| open_err("could not open the database file", &e))?;
        Self::init(conn)
    }

    /// An in-memory store, for tests and for `mav-replay`.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| open_err("could not open an in-memory database", &e))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| open_err("could not set pragmas", &e))?;

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|e| open_err("could not read the schema version", &e))?;

        if version > CURRENT_SCHEMA_VERSION {
            return Err(MavError::new(
                codes::STORAGE_NEWER_SCHEMA,
                "database schema is newer than this build understands",
            )
            .context(format!(
                "found v{version}, understand v{CURRENT_SCHEMA_VERSION}"
            )));
        }

        // Each migration is one transaction, and the version it reached is recorded inside it. A
        // step that fails halfway would otherwise leave its scratch tables behind and its version
        // unrecorded, so the next open would replay it onto a database it had already half
        // rewritten — the failure mode that turns one bad migration into an unopenable store.
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            let target = index as i64 + 1;
            if version >= target {
                continue;
            }
            let applied = conn.execute_batch("BEGIN IMMEDIATE").and_then(|()| {
                match migration {
                    Migration::Sql(sql) => conn.execute_batch(sql),
                    Migration::Rust(step) => step(&conn),
                }
                .and_then(|()| conn.pragma_update(None, "user_version", target))
                .and_then(|()| conn.execute_batch("COMMIT"))
            });
            if let Err(failure) = applied {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(
                    MavError::new(codes::STORAGE_MIGRATION, "a migration failed to apply")
                        .context(format!("migration to v{target}"))
                        .context(failure.to_string()),
                );
            }
        }

        Ok(Self { conn })
    }

    /// Run `work` inside one SQLite transaction: commit on `Ok`, roll back on `Err` so a partial
    /// burst can never persist (the M5 safe-ack invariant needs all-or-nothing writes). A rollback
    /// failure is appended to the original error's context rather than replacing it; SQLite drops
    /// an unfinished transaction when the connection closes, so no partial state survives either
    /// way.
    pub fn in_transaction<T>(&self, work: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| query_err("beginning a transaction", &e))?;
        match work(self) {
            Ok(value) => {
                self.conn
                    .execute_batch("COMMIT")
                    .map_err(|e| query_err("committing a transaction", &e))?;
                Ok(value)
            }
            Err(error) => match self.conn.execute_batch("ROLLBACK") {
                Ok(()) => Err(error),
                Err(rollback) => Err(error.context(format!("rollback also failed: {rollback}"))),
            },
        }
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|e| query_err("reading schema version", &e))
    }

    /// Append one scored sample. Idempotent on the natural key
    /// `(device, stream, device_time, seq, value)`, so re-syncing the same history lands once and
    /// two equal intervals with different `seq` both persist. That key is the same tuple
    /// `mav-timeline` deduplicates on, so the fast layer and the durable one cannot disagree.
    pub fn insert_sample(
        &self,
        device: DeviceId,
        sample: &Sample<RawValue>,
    ) -> Result<InsertOutcome> {
        let changed = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO sample \
                 (device_id, stream, device_time_ns, seq, value_tag, value_bits, wall_time_ns, \
                  placement, quality_score, quality_reason, provenance_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    device.get() as i64,
                    sample.kind.code(),
                    sample.device_time.as_nanos(),
                    sample.seq,
                    sample.value.tag(),
                    sample.value.key_bits() as i64,
                    sample.wall_time().map(WallTime::as_nanos),
                    sample.placement.code(),
                    f64::from(sample.quality.score),
                    sample.quality.reason.map(RejectReason::code),
                    sample.provenance.get() as i64,
                ],
            )
            .map_err(|e| query_err("inserting a sample", &e))?;
        Ok(if changed == 1 {
            InsertOutcome::Inserted
        } else {
            InsertOutcome::Duplicate
        })
    }

    /// Every stored sample of one stream, ordered by device time then sequence. Whole-stream reads
    /// are for export and replay; anything rendering a day uses [`Store::samples_between`], which
    /// the wall-time index serves as a range scan instead of a full scan.
    pub fn samples(&self, device: DeviceId, kind: StreamKind) -> Result<Vec<Sample<RawValue>>> {
        self.read_samples(
            "SELECT device_time_ns, seq, value_tag, value_bits, wall_time_ns, placement, \
                    quality_score, quality_reason, provenance_id \
             FROM sample WHERE device_id = ?1 AND stream = ?2 \
             ORDER BY device_time_ns, seq",
            params![device.get() as i64, kind.code()],
            kind,
        )
    }

    /// The samples of one stream placed inside `[from, until)` on the wall clock. Unplaced samples
    /// are not in any window and are therefore not returned.
    pub fn samples_between(
        &self,
        device: DeviceId,
        kind: StreamKind,
        from: WallTime,
        until: WallTime,
    ) -> Result<Vec<Sample<RawValue>>> {
        self.read_samples(
            "SELECT device_time_ns, seq, value_tag, value_bits, wall_time_ns, placement, \
                    quality_score, quality_reason, provenance_id \
             FROM sample \
             WHERE device_id = ?1 AND stream = ?2 \
               AND wall_time_ns >= ?3 AND wall_time_ns < ?4 \
             ORDER BY device_time_ns, seq",
            params![
                device.get() as i64,
                kind.code(),
                from.as_nanos(),
                until.as_nanos()
            ],
            kind,
        )
    }

    fn read_samples(
        &self,
        sql: &str,
        bindings: &[&dyn rusqlite::ToSql],
        kind: StreamKind,
    ) -> Result<Vec<Sample<RawValue>>> {
        let mut statement = self
            .conn
            .prepare_cached(sql)
            .map_err(|e| query_err("preparing sample read", &e))?;
        let rows = statement
            .query_map(bindings, sample_columns)
            .map_err(|e| query_err("reading samples", &e))?;
        let mut out = Vec::new();
        for row in rows {
            let columns = row.map_err(|e| query_err("reading a sample row", &e))?;
            out.push(row_to_sample(kind, columns)?);
        }
        Ok(out)
    }

    /// The single most recent stored sample of `kind` for `device`, ordered by `(device_time,
    /// seq)`, or `None` when the stream is empty. The host read models use this to surface a
    /// device's latest battery and wrist state without loading the whole stream.
    pub fn latest_sample(
        &self,
        device: DeviceId,
        kind: StreamKind,
    ) -> Result<Option<Sample<RawValue>>> {
        let row = self
            .conn
            .query_row(
                "SELECT device_time_ns, seq, value_tag, value_bits, wall_time_ns, placement, \
                        quality_score, quality_reason, provenance_id \
                 FROM sample WHERE device_id = ?1 AND stream = ?2 \
                 ORDER BY device_time_ns DESC, seq DESC LIMIT 1",
                params![device.get() as i64, kind.code()],
                sample_columns,
            )
            .optional()
            .map_err(|e| query_err("reading the latest sample", &e))?;
        row.map(|columns| row_to_sample(kind, columns)).transpose()
    }

    pub fn count_samples(&self, device: DeviceId, kind: StreamKind) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM sample WHERE device_id = ?1 AND stream = ?2",
                params![device.get() as i64, kind.code()],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n as u64)
            .map_err(|e| query_err("counting samples", &e))
    }

    pub fn upsert_provenance(&self, provenance: &Provenance) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO provenance \
                 (metadata_id, source_stream, quality, algorithm_id, algorithm_version, sample_count) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    provenance.metadata.get() as i64,
                    to_json(&provenance.source_stream)?,
                    f64::from(provenance.quality),
                    provenance.algorithm_id,
                    provenance.algorithm_version.to_string(),
                    provenance.sample_count,
                ],
            )
            .map_err(|e| query_err("writing provenance", &e))?;
        Ok(())
    }

    pub fn provenance(&self, metadata: MetadataId) -> Result<Option<Provenance>> {
        let row = self
            .conn
            .query_row(
                "SELECT source_stream, quality, algorithm_id, algorithm_version, sample_count \
                 FROM provenance WHERE metadata_id = ?1",
                params![metadata.get() as i64],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, u32>(4)?,
                    ))
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(query_err("reading provenance", &other)),
            })?;

        match row {
            None => Ok(None),
            Some((stream_json, quality, algorithm_id, version_str, sample_count)) => {
                let source_stream: StreamKind = from_json(&stream_json)?;
                let algorithm_version = version_str.parse::<Version>().map_err(|e| {
                    MavError::new(
                        codes::STORAGE_SERIALIZE,
                        "stored algorithm version is malformed",
                    )
                    .context(e.to_string())
                })?;
                Ok(Some(Provenance {
                    metadata,
                    source_stream,
                    quality: quality as f32,
                    algorithm_id,
                    algorithm_version,
                    sample_count,
                }))
            }
        }
    }

    /// Append an error to the durable journal, the ring log's persistent sibling.
    /// Write one derived daily snapshot, replacing any row for the same device and local day.
    /// Derived rows are replaceable by definition; raw samples are not, which is why only this
    /// table uses `INSERT OR REPLACE`.
    pub fn upsert_daily_snapshot(
        &self,
        device: DeviceId,
        local_day: i64,
        snapshot_json: &str,
        algorithms: &str,
        computed_ns: i64,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO daily_snapshot \
                 (device_id, local_day, snapshot_json, algorithms, computed_ns) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    device.get() as i64,
                    local_day,
                    snapshot_json,
                    algorithms,
                    computed_ns
                ],
            )
            .map_err(|e| query_err("writing a daily snapshot", &e))?;
        Ok(())
    }

    /// The stored snapshot for one local day, or `None` when it has not been computed.
    pub fn daily_snapshot(&self, device: DeviceId, local_day: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT snapshot_json FROM daily_snapshot WHERE device_id = ?1 AND local_day = ?2",
                params![device.get() as i64, local_day],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| query_err("reading a daily snapshot", &e))
    }

    /// Discard derived rows — for one device, or for every device when `device` is `None`. Safe by
    /// construction: recomputing from the retained samples reproduces them, which is the property a
    /// derived table is defined by.
    pub fn clear_derived(&self, device: Option<DeviceId>) -> Result<u64> {
        let filter = device.map(|id| id.get() as i64);
        let mut removed = 0usize;
        for table in ["daily_snapshot", "nightly_variability"] {
            removed += self
                .conn
                .execute(
                    &format!("DELETE FROM {table} WHERE ?1 IS NULL OR device_id = ?1"),
                    params![filter],
                )
                .map_err(|e| query_err("clearing derived rows", &e))?;
        }
        Ok(removed as u64)
    }

    /// Which streams hold a sample placed inside `[from, until)`. This is what the capability
    /// graph negotiates against, so a day that carries skin temperature says so rather than
    /// reporting the stream missing because nothing read it.
    pub fn streams_between(
        &self,
        device: DeviceId,
        from: WallTime,
        until: WallTime,
    ) -> Result<Vec<StreamKind>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT DISTINCT stream FROM sample \
                 WHERE device_id = ?1 AND wall_time_ns >= ?2 AND wall_time_ns < ?3 \
                 ORDER BY stream",
            )
            .map_err(|e| query_err("preparing stream census", &e))?;
        let rows = statement
            .query_map(
                params![device.get() as i64, from.as_nanos(), until.as_nanos()],
                |row| row.get::<_, u8>(0),
            )
            .map_err(|e| query_err("reading the stream census", &e))?;
        let mut out = Vec::new();
        for row in rows {
            let code = row.map_err(|e| query_err("reading a stream code", &e))?;
            out.push(StreamKind::from_code(code).ok_or_else(|| corrupt("stream kind"))?);
        }
        Ok(out)
    }

    /// Remember one night's variability so a sixty-night look-back does not re-derive sixty nights
    /// of beats. `None` records that the night was examined and held nothing usable.
    pub fn upsert_nightly_variability(
        &self,
        device: DeviceId,
        kind: StreamKind,
        local_day: i64,
        rmssd_ms: Option<f64>,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO nightly_variability \
                 (device_id, stream, local_day, rmssd_ms) VALUES (?1, ?2, ?3, ?4)",
                params![device.get() as i64, kind.code(), local_day, rmssd_ms],
            )
            .map_err(|e| query_err("writing nightly variability", &e))?;
        Ok(())
    }

    /// The remembered nights in `[first_day, last_day]`, as `(day, value)`. Days with no row are
    /// absent, which is how the caller knows which ones it still has to derive.
    pub fn nightly_variability(
        &self,
        device: DeviceId,
        kind: StreamKind,
        first_day: i64,
        last_day: i64,
    ) -> Result<Vec<(i64, Option<f64>)>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT local_day, rmssd_ms FROM nightly_variability \
                 WHERE device_id = ?1 AND stream = ?2 AND local_day BETWEEN ?3 AND ?4 \
                 ORDER BY local_day",
            )
            .map_err(|e| query_err("preparing nightly variability read", &e))?;
        let rows = statement
            .query_map(
                params![device.get() as i64, kind.code(), first_day, last_day],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<f64>>(1)?)),
            )
            .map_err(|e| query_err("reading nightly variability", &e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| query_err("collecting nightly variability", &e))
    }

    /// Forget the remembered nights a sync may have changed, so the next read re-derives them.
    pub fn forget_nightly_variability(
        &self,
        device: DeviceId,
        first_day: i64,
        last_day: i64,
    ) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM nightly_variability \
                 WHERE device_id = ?1 AND local_day BETWEEN ?2 AND ?3",
                params![device.get() as i64, first_day, last_day],
            )
            .map_err(|e| query_err("forgetting nightly variability", &e))?;
        Ok(())
    }

    pub fn record_error(&self, error: &MavError, created_ns: i64) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO error_journal \
                 (code, category, severity, message, context_json, created_ns) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    error.code,
                    category_str(error.category),
                    severity_str(error.severity),
                    error.message,
                    to_json(&error.context)?,
                    created_ns,
                ],
            )
            .map_err(|e| query_err("recording an error", &e))?;
        Ok(())
    }

    /// The most recent journal entries, newest first, at most `limit`.
    pub fn recent_errors(&self, limit: usize) -> Result<Vec<JournalEntry>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, code, category, severity, message, context_json, created_ns \
                 FROM error_journal \
                 ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| query_err("preparing journal read", &e))?;
        let rows = statement
            .query_map(params![limit as i64], |row| {
                Ok(JournalEntry {
                    id: row.get(0)?,
                    code: row.get(1)?,
                    category: parse_category(&row.get::<_, String>(2)?).map_err(|message| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                message,
                            )),
                        )
                    })?,
                    severity: parse_severity(&row.get::<_, String>(3)?).map_err(|message| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                message,
                            )),
                        )
                    })?,
                    message: row.get(4)?,
                    context: from_json_for_row(&row.get::<_, String>(5)?, 5)?,
                    created_ns: row.get(6)?,
                })
            })
            .map_err(|e| query_err("reading the journal", &e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| query_err("collecting journal rows", &e))
    }
}

/// The nine sample columns, in the order every sample query selects them. All read paths share one
/// row shape so a schema change cannot be applied to one and missed in the other.
type SampleColumns = (i64, u16, u8, i64, Option<i64>, u8, f64, Option<u8>, i64);

fn sample_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<SampleColumns> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn row_to_sample(kind: StreamKind, columns: SampleColumns) -> Result<Sample<RawValue>> {
    let (device_ns, seq, tag, bits, wall_ns, placement, score, reason, provenance) = columns;
    let value = RawValue::from_parts(tag, bits as u64).ok_or_else(|| corrupt("value width"))?;
    let reason = match reason {
        Some(code) => Some(RejectReason::from_code(code).ok_or_else(|| corrupt("reject reason"))?),
        None => None,
    };
    Ok(Sample {
        kind,
        device_time: DeviceTime::from_nanos(device_ns),
        placement: Placement::from_parts(placement, wall_ns.map(WallTime::from_nanos)),
        seq,
        value,
        quality: Quality {
            score: score as f32,
            reason,
        },
        provenance: MetadataId::new(provenance as u64),
    })
}

fn corrupt(what: &str) -> MavError {
    MavError::new(
        codes::STORAGE_SERIALIZE,
        "a stored sample column is not a value this build understands",
    )
    .context(what.to_owned())
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| {
        MavError::new(
            codes::STORAGE_SERIALIZE,
            "could not serialise a value for storage",
        )
        .context(e.to_string())
    })
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|e| {
        MavError::new(
            codes::STORAGE_SERIALIZE,
            "could not read a stored value back",
        )
        .context(e.to_string())
    })
}

fn open_err(what: &str, e: &rusqlite::Error) -> MavError {
    MavError::new(codes::STORAGE_OPEN, what.to_owned()).context(e.to_string())
}

fn query_err(what: &str, e: &rusqlite::Error) -> MavError {
    MavError::new(codes::STORAGE_QUERY, "storage operation failed")
        .context(what.to_owned())
        .context(e.to_string())
}

fn category_str(category: Category) -> &'static str {
    match category {
        Category::Transport => "transport",
        Category::Frame => "frame",
        Category::Decode => "decode",
        Category::Timeline => "timeline",
        Category::Storage => "storage",
        Category::Feature => "feature",
        Category::Analytic => "analytic",
        Category::Ml => "ml",
        Category::Ffi => "ffi",
        Category::Connector => "connector",
        Category::Internal => "internal",
    }
}

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Fatal => "fatal",
    }
}

fn parse_category(value: &str) -> std::result::Result<Category, String> {
    match value {
        "transport" => Ok(Category::Transport),
        "frame" => Ok(Category::Frame),
        "decode" => Ok(Category::Decode),
        "timeline" => Ok(Category::Timeline),
        "storage" => Ok(Category::Storage),
        "feature" => Ok(Category::Feature),
        "analytic" => Ok(Category::Analytic),
        "ml" => Ok(Category::Ml),
        "ffi" => Ok(Category::Ffi),
        "connector" => Ok(Category::Connector),
        "internal" => Ok(Category::Internal),
        other => Err(format!("unknown stored error category {other:?}")),
    }
}

fn parse_severity(value: &str) -> std::result::Result<Severity, String> {
    match value {
        "warning" => Ok(Severity::Warning),
        "error" => Ok(Severity::Error),
        "fatal" => Ok(Severity::Fatal),
        other => Err(format!("unknown stored error severity {other:?}")),
    }
}

fn from_json_for_row<T: DeserializeOwned>(text: &str, column: usize) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::raw::RawValue;

    fn sample(kind: StreamKind, device_ns: i64, seq: u16, value: RawValue) -> Sample<RawValue> {
        Sample {
            kind,
            device_time: DeviceTime::from_nanos(device_ns),
            placement: Placement::DeviceClock(WallTime::from_unix_seconds(1_752_600_000)),
            seq,
            value,
            quality: Quality::scored(1.0),
            provenance: MetadataId::new(1),
        }
    }

    #[test]
    fn migrations_apply_from_empty() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        // The tables exist: a count against each succeeds.
        assert_eq!(
            store
                .count_samples(DeviceId::new(1), StreamKind::HeartRate)
                .unwrap(),
            0
        );
        assert!(store.recent_errors(1).unwrap().is_empty());
    }

    #[test]
    fn latest_sample_returns_the_newest_by_device_time() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(3);
        // An empty stream is an honest None, never an error.
        assert!(store
            .latest_sample(device, StreamKind::BatterySoc)
            .unwrap()
            .is_none());
        // Insert out of order; the newest device_time must win regardless of insertion order.
        for (ns, pct) in [(3_000, 79.0), (1_000, 90.0), (2_000, 84.0)] {
            store
                .insert_sample(
                    device,
                    &sample(StreamKind::BatterySoc, ns, 0, RawValue::Converted(pct)),
                )
                .unwrap();
        }
        let latest = store
            .latest_sample(device, StreamKind::BatterySoc)
            .unwrap()
            .expect("a battery sample");
        assert_eq!(latest.device_time, DeviceTime::from_nanos(3_000));
        assert_eq!(latest.value, RawValue::Converted(79.0));
        // A different kind on the same device is isolated.
        assert!(store
            .latest_sample(device, StreamKind::WristState)
            .unwrap()
            .is_none());
    }

    #[test]
    fn latest_sample_breaks_device_time_ties_by_seq() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(4);
        for seq in [0u16, 2, 1] {
            store
                .insert_sample(
                    device,
                    &sample(
                        StreamKind::WristState,
                        5_000,
                        seq,
                        RawValue::U8(seq as u8 % 2),
                    ),
                )
                .unwrap();
        }
        let latest = store
            .latest_sample(device, StreamKind::WristState)
            .unwrap()
            .expect("a wrist sample");
        assert_eq!(latest.seq, 2);
    }

    #[test]
    fn refuses_newer_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION + 1)
            .unwrap();
        let err = Store::init(conn).err().unwrap();
        assert_eq!(err.code, codes::STORAGE_NEWER_SCHEMA);
    }

    #[test]
    fn a_failed_transaction_leaves_zero_rows() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(7);
        let first = sample(StreamKind::HeartRate, 1_000, 0, RawValue::U8(62));
        let second = sample(StreamKind::HeartRate, 2_000, 0, RawValue::U8(63));
        let error = store
            .in_transaction(|txn| -> Result<()> {
                assert_eq!(
                    txn.insert_sample(device, &first).unwrap(),
                    InsertOutcome::Inserted
                );
                let _ = txn.insert_sample(device, &second)?;
                Err(MavError::new(
                    codes::STORAGE_QUERY,
                    "injected failure on record two",
                ))
            })
            .unwrap_err();
        assert_eq!(error.code, codes::STORAGE_QUERY);
        assert_eq!(
            store.count_samples(device, StreamKind::HeartRate).unwrap(),
            0
        );
    }

    #[test]
    fn a_committed_transaction_is_durable() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(7);
        let inserted = store
            .in_transaction(|txn| {
                let mut inserted = 0;
                for (ns, bpm) in [(1_000, 62u8), (2_000, 63u8)] {
                    let s = sample(StreamKind::HeartRate, ns, 0, RawValue::U8(bpm));
                    if txn.insert_sample(device, &s)? == InsertOutcome::Inserted {
                        inserted += 1;
                    }
                }
                Ok(inserted)
            })
            .unwrap();
        assert_eq!(inserted, 2);
        assert_eq!(
            store.count_samples(device, StreamKind::HeartRate).unwrap(),
            2
        );
    }

    #[test]
    fn sample_insert_is_idempotent() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(7);
        let s = sample(StreamKind::HeartRate, 1_000, 0, RawValue::U8(62));
        assert_eq!(
            store.insert_sample(device, &s).unwrap(),
            InsertOutcome::Inserted
        );
        assert_eq!(
            store.insert_sample(device, &s).unwrap(),
            InsertOutcome::Duplicate
        );
        assert_eq!(
            store.count_samples(device, StreamKind::HeartRate).unwrap(),
            1
        );
    }

    #[test]
    fn distinct_seq_samples_both_persist() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(7);
        // Two equal RR intervals in the same second, distinguished only by seq.
        let a = sample(StreamKind::RrInterval, 2_000, 0, RawValue::U16(812));
        let b = sample(StreamKind::RrInterval, 2_000, 1, RawValue::U16(812));
        assert_eq!(
            store.insert_sample(device, &a).unwrap(),
            InsertOutcome::Inserted
        );
        assert_eq!(
            store.insert_sample(device, &b).unwrap(),
            InsertOutcome::Inserted
        );
        assert_eq!(
            store.count_samples(device, StreamKind::RrInterval).unwrap(),
            2
        );
    }

    #[test]
    fn samples_round_trip_exactly() {
        let store = Store::open_in_memory().unwrap();
        let device = DeviceId::new(3);
        let written = sample(StreamKind::HeartRate, 5_000, 0, RawValue::U8(71));
        store.insert_sample(device, &written).unwrap();
        let read = store.samples(device, StreamKind::HeartRate).unwrap();
        assert_eq!(read, vec![written]);
    }

    #[test]
    fn provenance_links_metric_to_source() {
        let store = Store::open_in_memory().unwrap();
        let provenance = Provenance {
            metadata: MetadataId::new(42),
            source_stream: StreamKind::RrInterval,
            quality: 0.9,
            algorithm_id: "rmssd".to_owned(),
            algorithm_version: Version::new(1, 2, 3),
            sample_count: 128,
        };
        store.upsert_provenance(&provenance).unwrap();
        let read = store.provenance(MetadataId::new(42)).unwrap().unwrap();
        assert_eq!(read, provenance);
        assert!(store.provenance(MetadataId::new(99)).unwrap().is_none());
    }

    #[test]
    fn error_journal_keeps_entries_newest_first() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_error(&MavError::new(codes::STORAGE_QUERY, "first"), 100)
            .unwrap();
        store
            .record_error(
                &MavError::new(codes::TIMELINE_IMPLAUSIBLE_TIMESTAMP, "second"),
                200,
            )
            .unwrap();
        let recent = store.recent_errors(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "second");
        assert_eq!(recent[0].code, codes::TIMELINE_IMPLAUSIBLE_TIMESTAMP);
        assert_eq!(recent[0].category, Category::Timeline);
        assert_eq!(recent[0].severity, Severity::Error);
    }

    #[test]
    fn connector_error_category_round_trips_through_the_journal() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_error(
                &MavError::new(codes::CONNECTOR_TRUST_SIGNATURE_INVALID, "bad signature"),
                100,
            )
            .unwrap();
        let recent = store.recent_errors(1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].category, Category::Connector);
        assert_eq!(recent[0].code, codes::CONNECTOR_TRUST_SIGNATURE_INVALID);
    }

    /// A v1 database, built exactly as the shipped v1 migration did, so the upgrade path is
    /// exercised against the encoding real installs actually hold.
    fn legacy_v1(path: &std::path::Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
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
                metadata_id INTEGER PRIMARY KEY, source_stream TEXT NOT NULL, quality REAL NOT NULL,
                algorithm_id TEXT NOT NULL, algorithm_version TEXT NOT NULL,
                sample_count INTEGER NOT NULL
            );
            CREATE TABLE error_journal (
                id INTEGER PRIMARY KEY AUTOINCREMENT, code INTEGER NOT NULL, category TEXT NOT NULL,
                severity TEXT NOT NULL, message TEXT NOT NULL, context_json TEXT NOT NULL,
                created_ns INTEGER NOT NULL
            );
            INSERT INTO sample VALUES
                (7, '\"rr_interval\"', 1000, 0, '{\"u16\":812}', 1752624000000000000, 1.0, NULL, 1),
                (7, '\"rr_interval\"', 2000, 0, '{\"u16\":820}', 1752624001000000000, 1.0,
                 '\"implausible_timestamp\"', 1),
                (7, '\"heart_rate\"', 3000, 0, '{\"u8\":62}', 1752624002000000000, 1.0, NULL, 1);
            PRAGMA user_version = 1;
            ",
        )
        .unwrap();
    }

    /// The bug a real phone found. Before the interval split there was one interval stream and
    /// every producer of it was optical, so carrying `rr_interval` across literally relabelled a
    /// wrist strap's beats as electrocardiography — and licensed the one claim this project exists
    /// to keep honest.
    #[test]
    fn intervals_recorded_before_the_split_migrate_as_optical() {
        let directory = std::env::temp_dir().join(format!("mav-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("legacy.sqlite");
        let _ = std::fs::remove_file(&path);
        legacy_v1(&path);

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

        let device = DeviceId::new(7);
        assert_eq!(
            store.count_samples(device, StreamKind::RrInterval).unwrap(),
            0
        );
        let optical = store.samples(device, StreamKind::PulseInterval).unwrap();
        assert_eq!(optical.len(), 2);
        assert_eq!(optical[0].value, RawValue::U16(812));
        assert_eq!(
            store.count_samples(device, StreamKind::HeartRate).unwrap(),
            1
        );

        // The old timestamp flag was a placement, not a value judgement, so the value survives
        // usable and the clock trouble is recorded separately.
        assert!(optical[1].quality.is_usable());
        assert_eq!(optical[1].quality.reason, None);
        assert!(matches!(
            optical[1].placement,
            Placement::CaptureFallback(_)
        ));
        assert!(optical[0].placement.is_trusted());

        drop(store);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A migration that fails partway must leave nothing behind. Reopening replays it from the
    /// version it actually reached, so a broken step is one bad open rather than a store nobody
    /// can open again.
    #[test]
    fn a_failed_migration_leaves_the_database_where_it_was() {
        let directory = std::env::temp_dir().join(format!("mav-rollback-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("legacy.sqlite");
        let _ = std::fs::remove_file(&path);
        legacy_v1(&path);

        {
            // A table the v3 rebuild is about to create: its CREATE now collides and the whole
            // step has to roll back.
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE sample_v3 (blocker INTEGER);")
                .unwrap();
        }

        let Err(failure) = Store::open(&path) else {
            panic!("a colliding table must fail the migration")
        };
        assert_eq!(failure.code, codes::STORAGE_MIGRATION);

        let conn = rusqlite::Connection::open(&path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2, "the version records only what actually landed");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM sample", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 3, "the original table is untouched");

        drop(conn);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
