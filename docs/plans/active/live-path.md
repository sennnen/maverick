# Live-path lane — bugs on the running pipeline, then structure

This lane fixes defects on the path that actually executes today — connector events in,
samples admitted, scored, ordered, stored, surfaced over FFI — and then decomposes the
oversized host and wires the observability crate that the plan calls milestone-zero work.
Every packet is in this repository.

The audit found the live path green in CI while broken in behaviour: battery and wrist
telemetry can never reach the app because the quality gate zeroes them; the timeline's
duplicate signal is computed and thrown away; the clock-correction machinery promised by
ADR-004 exists, is exported, and has no callers; and the session dedup memory grows without
bound. None of these had a test that could fail, which is the deeper defect this lane
corrects.

Ordering: LP-P1 through LP-P4 in order (LP-P2 and LP-P4 touch the same file region). LP-P5
strictly after LP-P2, LP-P4, and WF-P7's maverick-side touch. LP-P6 after LP-P5. The lane
exits when all six packets are done and the new integration tests demonstrate telemetry,
accounting, and clock behaviour end to end.

---

## Packet LP-P1: Exact-on-wire kinds score in SQI

**Owns:** `core/crates/mav-sqi/src/lib.rs`, its tests, one integration test in
`core/crates/mav-ffi/tests/`, `docs/pipeline.md` SQI stage.

**Must not touch:** `mav-model` (no frozen-type change without an ADR), the FFI gate itself.

**Contract:** `score_sample` scores only HeartRate and RrInterval; every other kind returns
`Quality::unassessed()` — score 0.0 (`mav-sqi/src/lib.rs:24-29`,
`mav-model/src/stream.rs:81-86`). The FFI's `bounded_sample` drops any sample with score
≤ 0 (`mav-ffi/src/lib.rs:431`). The combination makes `connector_telemetry`'s
`battery_percent` and `on_wrist` structurally always `None`, regardless of what a connector
emits. The design already intended exact-on-wire kinds to carry `Quality::exact()`
(`stream.rs:70-77`); SQI never applies it. Change `score_sample` so direct wire readouts —
BatterySoc (range-gated 0..=100), WristState, Gravity, the raw-counts kinds, SpO2/respiratory
raws — get full-score quality via the existing constructor. Genuinely analytic inputs stay
unassessed.

**Tests first:** a battery sample scores 1.0 (unit, red today); a committed battery
WireSample surfaces as `battery_percent: Some(..)` through `MavRuntime::connector_telemetry`
(integration, red today — this is the missing test that let the defect survive).

**Exit:** the full repo gate.

**Status: done.** `score_sample` now sorts streams into measured signals (range-gated), exact-on-wire
readouts (`Quality::exact()` inside the range the reading can occupy, rejected outside it), and raw
counts (still unassessed). The missing test exists: `telemetry_survives_the_quality_stage_it_actually_passes_through`
runs samples through `score_batch` before storing them, which the old telemetry test did not.

---

## Packet LP-P2: Commit accounting — dedup outcome and provenance

**Owns:** the `commit_samples` region of `core/crates/mav-engine/src/connector_host.rs`, its
tests, `docs/pipeline.md` accounting paragraph.

**Must not touch:** the ABI acknowledgement semantics (documented, not changed), `mav-store`
schema.

**Contract:** `connector_host.rs:1025` reads
`let _ = self.timeline.insert(scored) == TimelineInsertOutcome::Duplicate;` — the outcome is
computed and discarded, so duplicates are neither counted nor logged, violating the
never-drop-silently rule. Provenance is pushed for every emitted sample (`:1016`, upserted at
`:1031`) including ones the timeline dedups away, leaving orphan provenance rows.
`SamplesCommitted { count }` reports the emitted count, not the persisted one, and the boolean
`store.insert_sample` returns at `:1034` is also discarded. Capture the insert outcome; push
provenance only for `Inserted` samples; carry a per-commit duplicate count into the lifecycle
and trace surface; keep the ABI acknowledgement equal to the emitted count — acknowledgement
means received and safely handled, duplicates included — and write that sentence into
`docs/pipeline.md`; fold the store boolean into the same accounting.

**Tests first:** committing the same batch twice yields provenance rows exactly once and a
duplicate count equal to the batch length on the second pass (red today).

**Exit:** the full repo gate.

**Status: done.** `commit_samples` returns `CommitAccounting { emitted, persisted, duplicate }`.
Provenance is written only for samples the timeline accepted; the store's `InsertOutcome` folds into
the same accounting; duplicates are journalled under the new code 11061 and totalled on the lifecycle
snapshot (and over FFI). The ABI acknowledgement stays the emitted count, and `docs/pipeline.md` says
why.

---

## Packet LP-P3: Bounded timeline dedup memory

**Owns:** `core/crates/mav-timeline/src/lib.rs` (windowed dedup), its tests, one engine
integration test, `docs/adr/ADR-021.md`, ADR index, `docs/pipeline.md` layering paragraph.

**Must not touch:** the store's natural-key dedup.

**Contract:** `Timeline.seen` deliberately survives `drain_ordered`
(`mav-timeline/src/lib.rs:78-89`) and one Timeline lives for a whole session
(`connector_host.rs:161,218`), so a multi-day historical backfill — M5 is a completed
milestone — holds a dedup key for every sample ever inserted. Add
`Timeline::with_window(max_keys)`: HashSet plus VecDeque FIFO eviction, default 65,536 keys.
The store's `INSERT OR IGNORE` on its natural key remains the durable dedup layer (which is
why LP-P2 must surface its boolean); the window is the fast path, and a cross-window duplicate
is still rejected at the store. ADR-021 records the two-layer design.

**Tests first:** eviction order (oldest key evicted first, re-insert after eviction is
accepted by the timeline and still rejected by the store); an engine-level test replaying a
batch larger than the window twice persists each sample exactly once.

**Exit:** the full repo gate.

**Status: done.** `Timeline::with_window(max_keys)` — HashSet plus VecDeque FIFO eviction, default
65,536 — with ADR-021 recording the two-layer design. An engine test replays a batch larger than the
window twice and each sample still persists exactly once.

---

## Packet LP-P4: Wire ClockMap

**Owns:** `core/crates/mav-timeline/src/lib.rs` (`place_on_wall_with`), the clock-anchoring
region of `core/crates/mav-engine/src/connector_host.rs`, their tests,
`docs/adr/ADR-022.md`, ADR index, `docs/pipeline.md` placement stage.

**Must not touch:** `mav-model::time` (the types are right; they are just unused),
`mav-store` (no clock tables yet — persistence is deferred).

**Contract:** `place_on_wall` (`mav-timeline/src/lib.rs:96-106`) reinterprets device time as
wall time — directly contradicting `mav-model/src/time.rs:1-8` — and collapses implausible
clocks to the capture instant, destroying inter-sample deltas. `ClockMap`, `ClockSegment`,
`to_wall`, and `anchored` (`time.rs:69-118`) exist, are re-exported, and have zero callers;
ADR-004 promises correction as a stored mapping. The oracle algorithm is
`whoop-rs/crates/whoop-client/src/clock.rs`: trust an offset of at most one day; snap a stale
RTC to a 5-minute grid.

Add `place_on_wall_with(map: &ClockMap, sample, capture)`: a plausible device time is trusted
as-is; an implausible one goes through `map.to_wall(..)`; capture fallback only when the map
has no covering segment; corrected samples keep their raw device timestamp and carry the
existing reason marker. The ConnectorHost holds a per-session `ClockMap`: the first event that
pairs a host wall time with an implausible device time anchors
`ClockMap::anchored(device, wall)` snapped to the 5-minute grid; a plausible device clock
anchors identity. Cross-session persistence of segments is explicitly deferred and recorded in
ADR-022.

**Tests first:** the load-bearing one — two samples 10 seconds apart in device time under a
1970-era RTC come out 10 seconds apart on the wall (red today: both collapse to the capture
instant); plausible clocks unchanged; a sample arriving before the first anchor falls back to
capture with the reason recorded.

**Exit:** the full repo gate.

**Notes:** sequenced after LP-P2 — same file region.

**Status: done.** `place_on_wall_with(map, sample, capture)` and `anchor_from(device, capture)` in
mav-timeline, with `Placement::Corrected` as a third outcome; the host holds a per-session `ClockMap`
anchored on the first implausible device time, snapped to the five-minute grid. ADR-022 records the
deferral of cross-session persistence. The load-bearing test: two samples ten seconds apart under a
1970 RTC come out ten seconds apart.

---

## Packet LP-P5: Decompose the connector host

**Owns:** `core/crates/mav-engine/src/connector_host.rs` becoming
`core/crates/mav-engine/src/connector_host/{mod,manifest,lifecycle,actions,admission,trace}.rs`,
plus module-seam unit tests the extraction exposes.

**Must not touch:** the crate's public API (re-exported unchanged from `lib.rs`), any other
crate, behaviour of any kind.

**Contract:** `connector_host.rs` is 1,857 lines in one file: manifest validation, lifecycle
simulation, action queueing, state staging and commit, sample admission with SQI, timeline,
and store, and trace hashing. Split it along those seams into the six modules named above; no
new crates. The stream contract lands in `admission.rs` (coordinate with WF-P7, which touches
it). This is a behaviour-preserving move: every existing test stays green without
modification.

**Tests first:** none required to begin — the gate is that the move changes no test — and add
seam-level unit tests where extraction makes a previously untestable boundary reachable.

**Exit:** the full repo gate, with zero test-file diffs beyond added seam tests.

**Notes:** strictly after LP-P2, LP-P4, and WF-P7's maverick-side edit; those touch the same
file and must not race a rename.

**Status: done.** `connector_host.rs` (1,857 lines) is now `connector_host/{mod,manifest,lifecycle,actions,admission,trace,tests}.rs`,
largest non-test file 289 lines. Behaviour-preserving: no test assertion changed. Sibling-visible
methods became `pub(super)`; the type's public API is unchanged. **Beyond the packet as written:** the
test module moved to its own file too, which is what took `mod.rs` from 1,158 lines to 479.

---

## Packet LP-P6: Wire mav-obs

**Owns:** the Tap plumbing in `core/crates/mav-engine/src/connector_host/`, the ring-log and
report-bundle construction in `core/crates/mav-ffi/src/lib.rs`, the new allowed edge in
`docs/architecture.md` and `tools/check_deps.py`, `core/crates/mav-engine/Cargo.toml`, tests.

**Must not touch:** `mav-obs` internals (the crate is complete; it is merely unreachable).

**Contract:** `mav-obs` implements the Tap trait, ring log, trace hashes, and report bundle,
and is imported by no source file; `mav-ffi` declares the dependency and never uses it. The
plan's principle four — errors and observability are milestone zero — and the walk-back
requirement are unimplemented on the live path; the only live observability is the ad-hoc FNV
trace hash inside the host (`connector_host.rs:1260-1284`) and the error journal. Give the
ConnectorHost an `Option<Arc<dyn Tap>>`; call `on_stage` at the admission, SQI,
timeline-insert, and store boundaries — natural seams now that LP-P5 gave each a module.
`mav-ffi` constructs the ring log and exposes `export_report_bundle()`. Add the
mav-engine → mav-obs edge to `docs/architecture.md` and `tools/check_deps.py`; the declared
mav-ffi → mav-obs edge becomes real.

**Tests first:** a recording Tap sees the four stages in pipeline order for one committed
batch, with counts matching LP-P2's accounting.

**Exit:** the full repo gate.

**Status: done.** `ConnectorHost` takes an `Option<Arc<dyn Tap>>` and reports Decode, Sqi, Timeline,
and Store at the commit boundaries; `mav-ffi` owns a 512-entry `RingLog`, attaches `RingLogTap` to
every session, and exposes `export_report_bundle(limit)` carrying the app version, the live session's
trace hash, the commit totals, and the recent stage boundaries. The mav-engine → mav-obs edge is
declared in `docs/architecture.md` and `tools/check_deps.py`. A recording tap test asserts the four
stages in pipeline order with counts matching LP-P2's accounting.

---

## Decision log

- (empty — packets not yet started)
