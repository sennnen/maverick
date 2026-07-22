# Gardening lane — docs truth and dead-code removal

This lane makes the documents true again and removes everything the audit found dead:
leftover directories, phantom dependency edges, stale device comments in the generic core,
and — per the decision on record — the island crates `mav-frame` and `mav-codec`, which
remain unreachable even after the analytic spine lands because connectors do their own
framing and decoding inside Wasm (ADR-016/017). Nothing here changes behaviour; everything
here reduces the surface a reader must distrust.

Ordering: after the other lanes. G-P1 and G-P3 are independent; G-P2 last, because it
reconciles dependency edges to whatever the other lanes actually landed. The lane exits when
`tools/check_docs.sh` and `tools/check_deps.py` pass over documents that describe the
repository as it is.

---

## Packet G-P1: Docs truth

**Owns:** `docs/plans/active/M0.md` and `M1.md` (moving to `docs/plans/completed/` with
retros), `docs/plans/README.md`, `docs/PLAN.md` (repository layout and milestone table).

**Must not touch:** code, ADRs other than indexing.

**Contract:** M0 and M1 sit in `active/` although M0's own completion condition is met and
M1 opens with a note that ADR-017 and the WC lane superseded its instructions — move both to
`completed/` with honest retros (verify M0's exit lines; annotate M1's supersession rather
than pretending it completed as written). `docs/PLAN.md:96-108` lists only the original
twelve crates — add the five `mav-connector-*` crates. The two milestone tables disagree
about M7 (`PLAN.md:193` "host-mediated cloud source" versus `plans/README.md` "a manifest of
kind `cloud`") — reconcile to one sentence in both places. The WHOOP-P6/P8 packet tags
scattered through `mav-analytic` refer to a lane that has no plan file — record the series
in `plans/README.md` or a completed stub so the provenance is findable. Index the five new
lane files.

**Tests first:** not applicable; `tools/check_docs.sh` is the gate.

**Exit:** `tools/check_docs.sh`.

**Status: done.** M0 and M1 moved to `completed/` with honest retros — M0's exit verified, M1's
supersession by ADR-017 stated rather than papered over. `docs/PLAN.md` lists the five
`mav-connector-*` crates. M7 reads the same sentence in both tables. The `WHOOP-P*` tags are
explained in `plans/README.md` as provenance for a direct-commit series, with a note that no plan
file will be back-filled.

---

## Packet G-P2: Dead cleanup

**Owns:** deletion of the empty directories `maverick-connectors/whoop4`,
`maverick-connectors/whoop5` (repository top level), and
`core/connectors/mav-connector-whoop` here; the stale WHOOP wire comments in
`core/crates/mav-frame/src/crc.rs:1-4`; the ALLOWED table in `tools/check_deps.py`; the
parking of `mav-frame` and `mav-codec` out of the workspace with `docs/adr/ADR-025.md`; their
fixtures under `fixtures/standard/` and every doc reference to them; the `mav-obs` edge in
`core/crates/mav-ffi/Cargo.toml` per LP-P6's outcome.

**Must not touch:** live crates' code.

**Contract:** the empty directories contradict the documents that say they were deleted
(`docs/connector-audit.md:9,100`, the map in `CLAUDE.md`); remove them — the legacy-layout
guard in maverick-connectors' `tools/validate.py` stays as a tripwire. The comments in
`crc.rs:1-4` describe "the WHOOP wire" inside a crate ADR-012 requires to name no device.
`tools/check_deps.py` allows edges that do not exist (`:26` obs → store, `:31`
analytic → feature before AS-P2 made a direction real) — reconcile the table to the
post-lane reality, including the mav-engine → mav-obs edge LP-P6 added. Park `mav-frame` and
`mav-codec`: remove them from the workspace members with a short ADR-025 recording that they
are re-admittable from git history when a bundled SIG-profile path or host-side reassembly is
actually needed; deletion is complete — no orphan fixtures, doc references, tests, or Cargo
entries survive.

**Tests first:** not applicable; the gates and a scripted grep for every removed name are the
proof.

**Exit:** both repo gates; grep gates clean.

**Notes:** cross-repo — the two empty directories live in maverick-connectors and land as the
paired commit.

**Status: done.** Empty `whoop4`, `whoop5`, and `core/connectors/mav-connector-whoop` removed;
`mav-frame` and `mav-codec` moved to `attic/` with their fixtures under ADR-025, and `attic/README.md`
states the condition for re-admission. Workspace is 15 crates. `tools/check_deps.py` dropped both
crates and the phantom `mav-obs → mav-store` edge, and gained the real `mav-engine → mav-obs` one from
LP-P6. `CLAUDE.md`/`AGENTS.md`, `docs/PLAN.md`, `docs/architecture.md`, and `docs/pipeline.md` no
longer describe a host decode path. The stale WHOOP wire comments in `crc.rs` left the workspace with
the crate.

---

## Packet G-P3: Store read-path consolidation

**Owns:** `core/crates/mav-store/src/store.rs` — a single `row_to_sample` shared by
`samples()` (`store.rs:180-214`) and `latest_sample()` (`store.rs:235-269`).

**Must not touch:** the schema, migrations, write paths, tests (they stay green unchanged).

**Contract:** the two methods repeat the same seven-column row mapping; extract one private
helper. Behaviour-preserving.

**Tests first:** none new — the existing store tests are the harness; zero test diffs.

**Exit:** the full repo gate.

**Status: done.** One `row_to_sample` plus a shared `sample_columns` reader behind both `samples()`
and `latest_sample()`. Behaviour-preserving: zero test diffs, 13 store tests unchanged.

---

## Decision log

- (empty — packets not yet started)
