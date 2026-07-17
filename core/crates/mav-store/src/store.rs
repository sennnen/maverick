use crate::migrations::{CURRENT_SCHEMA_VERSION, MIGRATIONS};
use mav_model::error::{codes, Category, MavError, Result, Severity};
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::RawValue;
use mav_model::stream::{Quality, RejectReason, Sample, StreamKind};
use mav_model::time::{DeviceTime, WallTime};
use mav_model::version::Version;
use rusqlite::{params, Connection};
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

        for (index, sql) in MIGRATIONS.iter().enumerate() {
            let target = index as i64 + 1;
            if version < target {
                conn.execute_batch(sql).map_err(|e| {
                    MavError::new(codes::STORAGE_MIGRATION, "a migration failed to apply")
                        .context(format!("migration to v{target}"))
                        .context(e.to_string())
                })?;
            }
        }
        conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
            .map_err(|e| open_err("could not record the schema version", &e))?;

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
    /// two equal RR intervals with different `seq` both persist.
    pub fn insert_sample(
        &self,
        device: DeviceId,
        sample: &Sample<RawValue>,
    ) -> Result<InsertOutcome> {
        let stream = to_json(&sample.kind)?;
        let value = to_json(&sample.value)?;
        let reason = match sample.quality.reason {
            Some(r) => Some(to_json(&r)?),
            None => None,
        };
        let changed = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO sample \
                 (device_id, stream, device_time_ns, seq, value_json, wall_time_ns, \
                  quality_score, quality_reason, provenance_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    device.get() as i64,
                    stream,
                    sample.device_time.as_nanos(),
                    sample.seq,
                    value,
                    sample.wall_time.map(|w| w.as_nanos()),
                    f64::from(sample.quality.score),
                    reason,
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

    /// Read back the samples for one device and stream, ordered by device time then sequence. Used
    /// by the round-trip guarantee and by tests.
    pub fn samples(&self, device: DeviceId, kind: StreamKind) -> Result<Vec<Sample<RawValue>>> {
        let stream = to_json(&kind)?;
        let mut statement = self
            .conn
            .prepare(
                "SELECT device_time_ns, seq, value_json, wall_time_ns, quality_score, \
                        quality_reason, provenance_id \
                 FROM sample WHERE device_id = ?1 AND stream = ?2 \
                 ORDER BY device_time_ns, seq",
            )
            .map_err(|e| query_err("preparing sample read", &e))?;
        let rows = statement
            .query_map(params![device.get() as i64, stream], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, u16>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .map_err(|e| query_err("reading samples", &e))?;

        let mut out = Vec::new();
        for row in rows {
            let (device_time_ns, seq, value_json, wall_ns, score, reason_json, provenance) =
                row.map_err(|e| query_err("reading a sample row", &e))?;
            let value: RawValue = from_json(&value_json)?;
            let reason: Option<RejectReason> = match reason_json {
                Some(text) => Some(from_json(&text)?),
                None => None,
            };
            out.push(Sample {
                kind,
                device_time: DeviceTime::from_nanos(device_time_ns),
                wall_time: wall_ns.map(WallTime::from_nanos),
                seq,
                value,
                quality: Quality {
                    score: score as f32,
                    reason,
                },
                provenance: MetadataId::new(provenance as u64),
            });
        }
        Ok(out)
    }

    pub fn count_samples(&self, device: DeviceId, kind: StreamKind) -> Result<u64> {
        let stream = to_json(&kind)?;
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM sample WHERE device_id = ?1 AND stream = ?2",
                params![device.get() as i64, stream],
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
            wall_time: Some(WallTime::from_unix_seconds(1_752_600_000)),
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
}
