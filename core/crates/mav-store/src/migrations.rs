//! Forward-only numbered migrations. Each entry is the SQL that moves the schema from version
//! N-1 to N; `MIGRATIONS[0]` builds version 1 from an empty database. There are no down-migrations
//! (docs/storage.md): a wrong migration is fixed by the next one, never by rolling back.

/// The schema version this build understands. Opening a database recorded as newer than this is
/// refused rather than migrated.
pub const CURRENT_SCHEMA_VERSION: i64 = 1;

pub const MIGRATIONS: &[&str] = &[
    // v1 — the Milestone 1 slice.
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
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_count_matches_current_version() {
        assert_eq!(MIGRATIONS.len() as i64, CURRENT_SCHEMA_VERSION);
    }
}
