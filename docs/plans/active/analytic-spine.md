# Analytic-spine lane — wire the islands, retire the Kotlin analytics

This lane connects the analytics half of the core that today is built, tested, and reachable
from nothing: `mav-analytic` (~5,000 lines, 102 tests), `mav-feature`, and the engine's
recompute module all have zero callers, while the Android app ships its own live Kotlin
scorers and iOS shows honest empty shims. That state violates two standing rules at once —
"Rust owns analytics" and "a feature ships on both platforms or on neither." The decision on
record: wire the pipeline now, delete the Kotlin engines, and let any scorer that cannot pass
the ADR-009 admission bar become explicitly unavailable on both platforms rather than
silently ported.

Ordering: AS-P1 (docs) first, then AS-P2 → AS-P3 in order; AS-P4 may run beside AS-P2/P3;
AS-P5 and AS-P6 after AS-P3 and AS-P4; AS-P7 last. The lane starts only after LP-P1 through
LP-P4 have landed. The lane exits when both apps render the same DailySnapshot numbers from
the same fixture, the Android analytics package is gone with zero remaining references, and
capability negotiation is live over FFI.

---

## Packet AS-P1: The DailySnapshot contract

**Owns:** `docs/adr/ADR-024.md`, ADR index, the contract sections of `docs/analytics.md`,
`docs/pipeline.md`, and `docs/platform.md`.

**Must not touch:** code.

**Contract:** freeze, in documents, before code: the `DailySnapshot` record — HRV time-domain
values, readiness tier, recovery and strain labelled as compatibility estimates per their
admission status in `docs/analytics.md`, sleep summary, and an availability list carrying
`UnavailableReason` for everything not served; the trigger set — session end,
historical-complete, and app-driven midnight or on-demand refresh; the cache policy — the
existing `RecomputeCache`/`CacheKey`; and the timezone mechanism — platforms supply explicit
offset spans over FFI into the existing `Timezone::new(id, spans)` (no tzdata dependency in
Rust), with `RuntimeConfig.timezone_id` retained as a label.

**Tests first:** not applicable (documents), but every field named here must be asserted by a
fixture in AS-P2/AS-P3.

**Exit:** `tools/check_docs.sh`.

**Status: done.** ADR-024 freezes the record, the trigger set, the cache policy, and the timezone
mechanism, and is indexed. `docs/analytics.md` gains a DailySnapshot section under capability
negotiation stating that the availability list is part of the contract and that a platform never
substitutes its own number for an unavailable analytic.

---

## Packet AS-P2: The engine spine

**Owns:** new `core/crates/mav-engine/src/spine.rs`, `core/crates/mav-engine/Cargo.toml`
(new deps on `mav-feature`, `mav-analytic`), the matching edges in `docs/architecture.md` and
`tools/check_deps.py`, a forward-only migration in `core/crates/mav-store` for the derived
snapshot table, `docs/storage.md`, tests and a golden fixture day.

**Must not touch:** `mav-analytic` formulas (admitted algorithms change only with their own
fixtures), the FFI surface (AS-P3).

**Contract:** build the pipeline the architecture documents always promised: for each affected
`LocalDay`, read samples from the store, compute `mav-feature` primitives, run
`mav-analytic::capability` negotiation and the admitted analytics, and persist DailySnapshot
rows in a new derived table — forward-only migration, algorithm versions stamped on every row,
derived tables rebuildable by construction. The recompute module (`Timezone`, `LocalDay`,
`AffectedDays`, `RecomputeCache`) is finally constructed from FFI-supplied spans; the island
becomes live. Note `tools/check_deps.py:31` already phantom-allows analytic → feature —
reconcile the direction that becomes real.

**Tests first:** a golden fixture day produces pinned snapshot values; dropping the derived
table and recomputing produces identical rows; a cache hit short-circuits recomputation.

**Exit:** the full repo gate.

**Status: done.** `mav-engine/src/spine.rs` reads samples, buckets them by `LocalDay` through the
platform-supplied `Timezone`, runs `mav-feature::hr_summary` and `mav-analytic::time_domain`,
negotiates availability from the streams the day actually holds, and persists into the new
`daily_snapshot` table (migration v2, algorithm versions stamped). The recompute island is live:
`Timezone`, `LocalDay`, `AffectedDays`, and `RecomputeCache` all have callers. Seven tests pin a
golden day, prove drop-and-recompute is byte-identical, prove a cache hit reads no samples, and prove
the supplied offset decides which day a reading belongs to. Optical intervals are labelled
`pulse_rate_variability`, never HRV.

---

## Packet AS-P3: FFI exposure and parity

**Owns:** `core/crates/mav-ffi/src/lib.rs` (new calls, `RuntimeConfig` cleanup), the UDL/
generated-binding regeneration, the daily-snapshot hash fixture in the parity harness, both
apps' binding-consumption updates for the removed fields, `docs/platform.md`.

**Must not touch:** engine internals, app UI beyond the coordinated field removal.

**Contract:** expose `daily_snapshot(day)`, `analytic_availability()`, and
`set_timezone_spans(...)`. Resolve every dead `RuntimeConfig` field
(`mav-ffi/src/lib.rs:46-53`, only `database_path` is read today): keep `timezone_id` as the
label AS-P1 defines; wire `app_version` into the LP-P6 report bundle; remove
`transport_capacity` and `app_build` in a coordinated change with both apps in this packet.
The parity harness gains a daily-snapshot hash fixture asserted byte-identical on iOS and
Android.

**Tests first:** FFI integration — the golden day from AS-P2 returns the pinned snapshot over
the boundary; both platform test suites assert the same hash.

**Exit:** the full repo gate plus both platform test suites.

**Status: done.** `daily_snapshot(device, wall_ms)`, `analytic_availability(...)`,
`set_timezone_spans(id, spans)`, plus `heart_rate_zones`/`heart_rate_zone_for` from AS-P7.
`RuntimeConfig` lost `transport_capacity` and `app_build` in a coordinated change with both apps;
`timezone_id` stays as the ADR-024 label and `app_version` now stamps the report bundle. The FFI
test pins the parity hash `a220b57a8d4690b0` over a fixture day, asserts the PRV label, asserts
`recovery` reports `algorithm_not_admitted`, and asserts an empty span list is refused rather than
silently treated as UTC.

---

## Packet AS-P4: Kotlin scorer disposition audit and gap ports

**Owns:** any new modules in `core/crates/mav-analytic/src/` that the audit admits, their
tests and fixtures, the disposition table in `docs/analytics.md`.

**Must not touch:** the Kotlin files themselves (AS-P5 deletes them), admitted formulas.

**Contract:** map each Android scorer — `IllnessSignalEngine` (a 175-line z-score composite
with corroboration gating), `RestScorer`, `V5HealthSignals`, `CyclePhaseEngine` — onto the
existing `mav-analytic` modules (`hr_anomaly`, `stress`, `readiness`, `sleep` already cover
much of the ground; audit precisely and reuse before porting). For each genuine gap, port only
if its translated tests can genuinely fail per ADR-009; otherwise it becomes an explicit stub
reported unavailable through capability negotiation. Expected outcome: `CyclePhaseEngine` has
no reference or fixture and lands as unavailable — an accepted, visible regression, decided on
record, not a silent port. The deliverable includes a disposition table in `docs/analytics.md`
naming every scorer and its fate.

**Tests first:** per ported scorer, the translated tests observed red before the port; per
stub, a capability test asserting the exact `UnavailableReason`.

**Exit:** the full repo gate.

**Status: done.** Disposition table in `docs/analytics.md`. **The audit's finding changed the
packet:** none of the four scorers was reachable — `AppViewModel.days()` returned an empty list and
the signals flow was never published — so every one was computing over nothing. Deletion removed no
working feature. `Hrv.rmssd` and `IllnessWatch` were duplicates and went; `Zones`/`hrMaxTanaka`
became FFI calls into `mav-analytic::hr_zones`; `RestScorer`, `IllnessSignalEngine`, and
`CyclePhaseEngine` became the declared capabilities `SleepPerformance`, `IllnessRisk`, and
`CyclePhase`, each carrying the streams it needs so the reason shown is true. A second correction:
`AnalyticsModels.kt` and the iOS shims held *presentation* types (`StageSegment`, `UserProfile`,
`DetectedSleep`, `HypnogramMetrics`) mixed in with the scorers — those moved to `ui/`, they were not
deleted.

---

## Packet AS-P5: Android cutover

**Owns:** deletion of `apps/android/.../analytics/{IllnessSignalEngine,CyclePhaseEngine,RestScorer,V5HealthSignals}.kt`
with their models and tests; the rewires in `AuraRecoveryScreen.kt:191-194`,
`AuraSleepScreen.kt:78`, `AuraTodayScreen.kt:81`, `AppViewModel.kt:60-62`; unavailable-state
rendering from the availability list; `apps/android/README.md`.

**Must not touch:** `RouteMath.kt` (not a scorer; deliberately retained), iOS.

**Contract:** the four Kotlin engines are deleted completely and the four call sites consume
the FFI `DailySnapshot` and availability list instead. A card backed by an unavailable
analytic shows the reason from the core — never a platform substitute, per the platform lane's
honesty rule. Cleanup gate: a grep over the app proves zero remaining references to the
deleted package, its types, or its tests.

**Tests first:** Android UI/unit tests asserting the snapshot values and the unavailable
rendering; the grep gate scripted into the packet's exit.

**Exit:** Android test suite green; the grep gate clean; the app builds from a clean checkout.

**Status: done, compiled and tested.** Six Kotlin files deleted; `RouteMath.kt` retained. The four
call sites now read `vm.dailySnapshot`; the two `RestScorer` fallbacks are gone, because a local
fallback for a core metric is the violation, not a convenience. The Signals section renders
`AuraUnavailableCard` for `illnessrisk` and `cyclephase`. `AndroidConnectorManager` publishes the
platform's offset spans on runtime open and exposes `dailySnapshot`. Grep gate clean (the surviving
`setIllnessWatch` is a user preference, not the engine). `:app:compileDebugKotlin` and
`:app:testDebugUnitTest` both green under JDK 17.

---

## Packet AS-P6: iOS cutover

**Owns:** `apps/ios/Maverick/Model/AnalyticsShims.swift` replacement with FFI DailySnapshot
reads, the same unavailable-state rendering, `apps/ios/README.md`, iOS tests.

**Must not touch:** Android.

**Contract:** the shim types give way to decoded core records; rendering of unavailable
states is identical in information content to Android's. Exit condition for the pair of
packets: both apps show the same numbers from the same fixture.

**Tests first:** iOS tests asserting the fixture snapshot values and the shared hash from
AS-P3.

**Exit:** iOS test suite green; the cross-platform hash fixture matches.

**Status: written, not compiled — no Xcode on this machine.** The mirror is complete:
`AnalyticsShims.swift` keeps only `StageSegment`, `AppModel` publishes `dailySnapshot` in place of
the two engine results, the Signals section renders the new `AuraUnavailableCard` (same wording as
Android's, accessibility label included), `MavStore.runtimeConfig()` matches the trimmed
`RuntimeConfig`, and `ConnectorRuntimeWorker` gained `publishTimezoneSpans` and `dailySnapshot`.

Every modified file passes `swiftc -parse`, which is syntax only. **This machine has Command Line
Tools, not Xcode.app, so there is no iOS SDK and neither type-checking nor the test suite could
run.** The next session on a machine with Xcode must build and run `MaverickTests` before this
packet is called done, and assert the AS-P3 parity hash from the iOS side.

---

## Packet AS-P7: Platform dedup — zones and logical day

**Owns:** FFI exposure of `mav-analytic::hr_zones` and `LocalDay` bucketing; deletion of
`WorkoutZones.swift`, `WorkoutZones.kt`, and the platform `LogicalDay` implementations with
their mirrored parity tests; one FFI fixture test per platform replacing each mirrored pair;
the deferral note in `docs/platform.md`.

**Must not touch:** `RouteMath`, `Units`, `MavPresent` — formatting and locale are
platform-idiomatic; their move to core is deliberately deferred and the deferral is recorded,
not left implicit.

**Contract:** zone math and day bucketing are deterministic computation duplicated by hand in
two languages and defended only by mirrored test suites; both already exist in Rust. Expose
them; delete the duplicates completely (code, tests, references).

**Tests first:** per platform, one fixture test against the FFI values, red before the
exposure exists.

**Exit:** the full repo gate plus both platform suites; grep gates for the deleted types.

**Status: half done.** Zone math is exposed (`heart_rate_zones`, `heart_rate_zone_for`) and the
Kotlin `Zones` object is deleted; `AuraZoneMath` defers to the core. The `LocalDay` half is not
done — the platform `LogicalDay` implementations and `WorkoutZones.kt`'s percentage parsing are
still local, and `WorkoutZones` turns out to parse *imported* zone JSON rather than compute zones, so
it is not the duplicate the packet assumed. Both want their own packet against real imported data.

---

## Decision log

- **AS-P1 landed; AS-P2 through AS-P7 are not started, and the reason is worth recording.** AS-P2 and
  AS-P3 (the engine spine and its FFI surface) are ordinary Rust work that can be written and
  verified here. AS-P4 through AS-P7 cannot: they delete Kotlin, rewrite Swift, and assert against
  Android and iOS test suites, and neither Gradle nor Xcode is available in this environment. Writing
  them blind would mean shipping app changes whose test output nobody has read, against the standing
  rule that a gate is verified by reading its output rather than trusting it. The contract they build
  against is frozen, which is the part that had to happen first.
- **The freeze is deliberately narrow on timezones.** The platforms supply offset spans and nothing
  else. It is the one place a platform feeds an analytic input, and a span is a fact about the world
  rather than a judgement about health data — the distinction that keeps "Rust owns analytics" true
  while avoiding a tzdata dependency the phone already carries.
