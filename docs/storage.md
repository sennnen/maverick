# Storage

Maverick stores everything in one SQLite database per install, opened through rusqlite in WAL mode.
The schema is organised around a single idea: raw data is evidence and is never touched; everything
else is a computation over that evidence and can be thrown away and redone. Both codebases Maverick
learned from converged on this same three-tier pattern independently, which is about as strong an
endorsement as reverse-engineering work gets.

## The three tiers

**Tier one: raw evidence, append-only.** The bytes and samples as they arrived: raw frames
(optionally, as a bounded ring), samples, and events. Rows in these tables are inserted and never
updated or deleted. Raw payloads are content-addressed by their sha256, so the same bytes stored
twice deduplicate to one row and a stored payload can be verified against its own hash. Raw rows
carry a consent tag. Per-sample stream tables are keyed by `(device, ts, …)` with insert-or-ignore
semantics, which is what makes re-syncing the same history idempotent: a backfill replayed twice
lands exactly once.

**Tier two: decoded frames, regenerable.** The structured results of decoding tier one. Every row
in this tier stores the `parser_version` that produced it, because a decode is only reproducible if
you know which decoder ran. When a parser is fixed, the affected rows are regenerated from the raw
tier under the new version; the raw evidence needed to do that is, by tier-one rules, still there.

**Tier three: metrics and rollups, recomputed.** Features, metrics, and day-level rollups. These
are mutable, updated by UPSERT with idempotency guards, because a recompute trigger (a historical
sync finishing, a day closing at local midnight) may recompute a window that already has values.
The guards make recomputation safe to repeat: running the same recompute twice writes the same
rows.

The tiers are ordered by trust. Tier one is what the device said, and nothing in the system is
allowed to revise it. Tier two is what our current code thinks the device meant. Tier three is what
our current algorithms make of that. A bug in tier three costs a recompute; a bug in tier two costs
a re-decode; only losing tier one loses anything at all.

## Migrations

Migrations are forward-only, numbered, and applied automatically when the database is opened. The
current schema version lives in a schema-version pragma. There are no down-migrations: a migration
that turns out to be wrong is corrected by the next numbered migration, not by rolling back, because
a rollback that has to un-transform data is a data-loss mechanism wearing a safety costume. If the
database's recorded schema version is **newer** than the code understands, the store refuses to
open it, since writing to a schema the code does not know can only corrupt.

## Provenance

The provenance table is keyed by `MetadataId` and records, for each derived value: the source
stream, the quality that went in, the algorithm id, the algorithm version, and the sample count.
Every feature and metric carries a `MetadataId`, so provenance is by reference: the value on screen
points at a row that names exactly what produced it. This table is the storage half of the walk-back
requirement in [errors.md](errors.md), the chain from a metric back through features and samples to
frames and raw bytes. It is also what makes "which algorithm version computed this" answerable after
the fact, which matters every time an algorithm changes and old values need to be told apart from
new ones.

## The per-device KV table

There is a small key-value table scoped by device, and it exists for the codecs. A `DeviceCodec`
that learns something about its specific physical device (the gen4 skin-temp anchor being the
standing example, see [connectors.md](connectors.md)) stores it here. The table is codec-scoped in
both directions: codecs are the only writers, and a codec sees only its own device's keys. Learned
values live here rather than in tier three because they are not derived from stored samples by a
rerunnable algorithm; they are accumulated device state, and dropping them would genuinely lose
something, so they sit outside the disposable tiers.

## The round-trip guarantee

Derived data is disposable, and this is a tested guarantee rather than an aspiration: delete every
derived table, recompute from raw plus the recorded versions, and the result must be identical to
what was deleted. The test exists (see [testing.md](testing.md)) because the property decays the
moment anyone stops checking it. All it takes is one derived table quietly accumulating state that
is not derivable, or one computation reading the clock, and the recompute stops matching. While the
round-trip holds, a whole class of operations stays cheap and safe: fixing an algorithm, reversioning
a parser, recovering from a corrupted derived table. All of them reduce to "drop and recompute",
and the raw evidence guarantees the recompute has everything it needs.
