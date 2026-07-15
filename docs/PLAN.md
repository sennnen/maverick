# Maverick — master plan

This is the single most important document in the repository. It says what Maverick is, the constraints it is built under, the principles that shape every structural decision, the shape of the code, the shape of the data as it moves through the system, and the way the work is cut up so that a swarm of agents can execute it safely. Read it before doing any structural work. If something you are about to build contradicts this document, the document is wrong or your change needs an ADR, and either way you stop and raise it rather than quietly working around it.

## What Maverick is

Maverick (short: Mav; written lowercase as `mav` in crate names, the CLI, and code) is a from-scratch, cross-platform wearable-data platform for iOS and Android. BLE straps stream to the phone, and everything after that happens on the device: decoding the wire protocol, scoring signal quality, building a timeline, computing features and metrics, running any inference. Nothing leaves the phone. There is no server, no account, no cloud sync of health data. The straps we support first are the WHOOP 4.0 and the WHOOP 5.0/MG.

The decoding and analytics live in a shared Rust core that both platforms link through UniFFI bindings. The two apps are thin shells that render read-only snapshots the core hands them. A metric is computed once, in one language, and both platforms see the same number because they are running the same code. This is the central architectural bet, and most of the rest of the plan exists to protect it.

Maverick is a rewrite in spirit of an earlier lineage of wearable projects, but it is a new and unaffiliated project with its own code and its own decisions. The README carries a one-line note about that lineage at the bottom and nothing more.

The work is done by a swarm of AI coding agents steered by one human. That fact is not incidental. A swarm is fast and tireless and has no shared memory of yesterday's conversation, so it will cheerfully build the same thing twice, edit a file another agent is depending on, or write a test that passes because it asserts nothing. Every structural choice below is chosen to make that failure mode hard: frozen interfaces, mechanical gates that a machine checks on every change, documents that are the system of record rather than a summary of it, and units of work small enough that one agent can finish one without needing to hold the whole system in its head.

## The hardware-free constraint

We do not yet have any straps. A WHOOP 4.0 and a 5.0/MG are on order. Until they arrive, nobody on this project can hold a real device against real code, which means every protocol fact we work from is inferred: read out of two surveyed codebases, cross-checked against captured byte sequences, and reasoned about, but never once confirmed by watching a physical strap on a physical wrist produce the bytes we claim it produces.

This shapes the whole approach. Everything on the protocol side must be buildable and testable from captured fixtures and code-inferred facts. The primary tool is `mav-replay`, which feeds a capture file through the full pipeline and dumps every stage boundary to JSON, so an agent can develop and test a decoder without a radio in the room. The BLE state machine is pure Rust driven by injected events, so it is unit-testable without CoreBluetooth. Only the thin native transport shim that actually talks to the radio stays untested until a strap is in hand.

Every protocol fact carries a confidence tag, and those tags are load-bearing. `docs/protocol/whoop.md` is the ledger, and it uses five tags:

| tag | meaning |
|---|---|
| `[XVAL]` | both surveyed codebases agree; high confidence, still hardware-verify eventually |
| `[ONE]` | only one codebase asserts it; medium confidence |
| `[PROV]` | provisional, uncalibrated, or self-admittedly guessed; treat as an approximation |
| `[HW]` | can only be confirmed with a physical strap, which we do not have |
| `[CONFLICT]` | the two codebases disagree and it must be resolved on hardware |

A fact does not get to lose its tag by being repeated confidently in prose. It loses its tag when a real strap verifies it. When the hardware arrives, flipping every `[CODE-INFERRED]` fact to `[HW-VERIFIED]` is a checklist we already have written down (see the hardware epoch, below), not an archaeology project we start from scratch. That is the entire point of tagging as we go: the cost of being wrong now is a line edit later, not a silent decoder bug that ships.

## Guiding principles

Two earlier codebases were surveyed to seed this project, and a prior plan existed. A good deal of it was sound and is kept. Six things are deliberately changed, and the reasoning for each matters more than the change itself, because the reasoning is what tells a future agent whether a new situation falls under the same rule.

### What we keep

The Rust core with UniFFI bindings, both platforms always, is kept. Vertical-slice milestones with exit criteria rather than calendar dates are kept, because a swarm does not respond to deadlines and does respond to a gate that is either green or red. The canonical timeline that never invents data is kept. Signal quality as a first-class pipeline stage, scored on raw signals before anything is normalised, is kept. Capability negotiation, where each analytic declares what streams it needs and the engine works out what is available, is kept. Provenance on every derived value, versioning on every algorithm and every stored record, ML inference running natively with preprocessing in Rust, architecture decision records for anything structural, and the working assumption that existing behaviour is a specification to be matched rather than a suggestion, are all kept.

### What we change, and why

**1. No event buses. A synchronous typed pipeline instead.** The prior plan wired the system together with six publish/subscribe buses. We reject that. Pub/sub hides call order: you cannot read the code and know what runs when, replay stops being deterministic, and debugging becomes a matter of guessing which subscriber fired. That is precisely the environment in which an agent swarm produces spaghetti, because each agent adds a publisher or a subscriber that looks locally reasonable and the global behaviour drifts. In its place every stage is an ordinary function with the shape `fn run(input: StageIn, ctx: &mut StageCtx) -> Result<StageOut, MavError>`. The types at every stage boundary are frozen in `mav-model`. You get the same inspectability a bus was supposed to give you from a `Tap` trait that is invoked at each boundary, but the call graph stays deterministic and readable. Async lives in exactly one place: a single bounded channel from the native BLE layer into the entry of the pipeline.

**2. No forty algorithms up front. A fixture-admission rule instead.** The earlier work accreted around forty analytic engines, many of them speculative, whose tests proved only that the Swift and Kotlin versions agreed with each other. An algorithm lands in Maverick only when it has one of two things: a golden fixture derived from a real capture or a published reference implementation, or property and invariant tests that can genuinely fail. Anything without one of those stays an explicit stub, and capability negotiation reports it as unavailable rather than pretending it works. This kills speculative code before it is written and kills the always-green test that asserts nothing, which was the most common defect in both surveyed codebases. A parity test that shows two platforms agree is necessary but not sufficient: two platforms can be consistently wrong. A number is validated only against ground truth or a published reference; otherwise it is merely consistent, and it must be labelled provisional.

**3. Manifests are declarative data plus a small boxed-in codec, not pure data.** It is tempting to describe every device as a JSON file and have one generic decoder read it. Reality from the surveyed codebases says that does not hold: the WHOOP 4.0 needs a per-device learned skin-temperature anchor, handshakes are stateful, and some decodes carry memory across records. So `manifest.json` holds everything static — identity, GATT UUIDs, frame parameters, the packet map, field layouts, unit conversions, record versions, sensor configs — and a small `DeviceCodec` Rust trait holds only the logic that data cannot express. The codec is boxed in by its interface. It receives bytes, its own manifest, and a per-device key-value store for learned values like that skin-temp anchor, and it returns frames and samples. It cannot reach storage, the network, analytics, or any other device. Adding a device is one manifest and, when the logic genuinely needs it, one small codec crate. The core is not touched.

**4. Errors and observability are milestone zero, not the last milestone.** The prior plan left error handling and forensics to the end. For a swarm that is backwards. A swarm with no forensics ships silent corruption, and by the time you notice, you cannot tell which of a hundred merged changes did it. So the error taxonomy, tracing, the error journal, and the user-facing report bundle are all M0 deliverables. From the first milestone, nothing is dropped silently: every discarded packet, sample, or frame logs a stable error code and a reason.

**5. Milestones decompose into work packets, and the packet is the unit of agent work.** The prior plan already noticed that agents degrade on broad, open-ended tasks. A work packet makes the narrow task structural rather than a matter of discipline. Each packet names the files it owns, the files it must not touch, the exact contract it implements, the failing tests it writes first, and the commands that must pass before it is done. One packet is one commit. An agent that stays inside its packet cannot collide with another agent, because their owned files do not overlap. Section 10 of this document defines the template and the protocol in full.

**6. The mock second device comes early, right after the first vertical slice.** The connector abstraction — manifest plus codec — is the thing that lets us add devices without editing the core. An abstraction that has only ever seen one device has not been tested; it has been shaped to fit that one device. So immediately after the first real slice works end to end, we introduce a mock device with a deliberately different frame format and a codec that needs its own per-device state, and we require it to stream through the pipeline with no edits to the core at all. If that requires a core edit, the abstraction was wrong, and we would much rather learn that at device number two than at device number six.

## Repository layout

The tree below is the whole repository. Each entry has a one-line description of what lives there and, by implication, what does not. Ownership boundaries in the crate list are the same boundaries work packets are cut along.

```
maverick/
  CLAUDE.md            agent map, ~100 lines; identical twin of AGENTS.md
  AGENTS.md            same content as CLAUDE.md; CI fails if the two differ
  README.md            short and human: what mav is, current status, one-line lineage note
  docs/
    PLAN.md            this file: principles, milestone table, packet protocol
    architecture.md    system map, crate ownership, the allowed dependency edges
    pipeline.md        stage-by-stage contracts of the data pipeline
    connectors.md      manifest schema, the DeviceCodec contract, how to add a device
    protocol/
      whoop.md         every known WHOOP protocol fact, each with a confidence tag
    testing.md         test policy: fixture rules, property tests, parity, anti-faux rules
    errors.md          the error taxonomy, codes, logging, the user report bundle
    storage.md         append-only schema, migrations, provenance tables
    ml.md              the native-inference boundary, Rust preprocessing, golden vectors
    adr/               ADR-001 onward: short records of structural decisions
    plans/
      active/          per-milestone packet breakdowns currently in flight
      completed/       milestone files moved here with a retro when finished
  skills/
    work-packet/SKILL.md          how to execute one packet start to finish
    golden-fixtures/SKILL.md      how to generate and version a golden fixture
    connector-authoring/SKILL.md  how to write a manifest and, if needed, a codec
    doc-gardening/SKILL.md        how to keep docs and code from drifting apart
  core/
    Cargo.toml         the Rust workspace
    crates/
      mav-model/       frozen types: ids, time, streams, samples, quality, errors, versions
      mav-frame/       CRC 8/16/32, the reassembler, the TypedReader
      mav-codec/       the DeviceCodec trait, manifest types, the device registry
      mav-timeline/    ordering, dedup, clock correction, historical merge
      mav-sqi/         signal quality scoring
      mav-feature/     primitive, derived, and aggregate features
      mav-analytic/    the metric DAG and capability negotiation
      mav-store/       rusqlite, append-only storage, provenance, the error journal
      mav-obs/         tracing setup, the Tap trait, the report bundle
      mav-engine/      orchestration: triggers, the task graph, caching
      mav-ffi/         the UniFFI facade both apps link
      mav-replay/      binary: run a capture file through the pipeline, for hardware-free dev
  connectors/
    whoop4/manifest.json   plus a codec crate when logic requires it
    whoop5/manifest.json
    mock/manifest.json     the fake device used to prove the abstraction
  fixtures/            golden fixtures, versioned; README explains naming and the rules
  apps/
    ios/README.md      binding shell; the real app is a later milestone
    android/README.md
  tools/
    check_docs.sh      links resolve, CLAUDE.md == AGENTS.md, every plan is indexed
    check_deps.py      crate dependency edges match architecture.md
  .github/workflows/ci.yml
  rust-toolchain.toml
  rustfmt.toml
  .gitignore
```

The dependency edges between crates are not a free-for-all. They are written down in `architecture.md`, and `tools/check_deps.py` fails the build if the actual edges in `Cargo.toml` files stop matching that document. `mav-model` sits at the bottom and depends on nothing of ours; everything depends on it. A codec cannot depend on storage or analytics, which is how the boxed-in property from principle 3 is enforced mechanically rather than by good intentions.

## The data pipeline

Data moves through a fixed sequence of stages. Each stage is a synchronous function with frozen input and output types, and each writes to the `Tap` at its boundary so the whole run can be observed without changing behaviour. The order is the order; there is no bus deciding it at runtime.

```
native BLE bytes
  -> Acquisition   state machine plus transport: reassembly, CRC, decrypt, command matching
  -> Frames        validated frames; every rejected fragment logged with a reason code
  -> Decode        manifest field layouts plus codec: Frame -> RawSample batches
  -> SQI           raw signals scored before normalisation: value plus quality plus reason
  -> Timeline      order, dedup, clock-correct, merge historical; never interpolate, never
                   mutate a raw timestamp (corrections are stored as mappings, not edits)
  -> Store         append-only
  -> Features      primitive -> derived -> aggregate; a provenance metadata id on each
  -> Predictions   Rust preprocessing -> native CoreML/TFLite -> prediction plus confidence
  -> Metrics       recovery, strain, sleep quality, and the rest
  -> Snapshots     immutable read models the UI queries over FFI
```

Acquisition is a state machine with these states: Disconnected, Scanning, Connecting, Authenticating, Configuring, Streaming, HistoricalSync, Idle. Every transition is logged. The command layer matches requests to responses by sequence number, times out, and retries a bounded number of times (three attempts, exponential backoff). Because the machine is pure Rust fed by injected events, the whole of it is testable without a radio.

The timeline stage has one rule that history taught us the hard way, and it is written into `pipeline.md` as an invariant test: two equal RR intervals in the same second are two distinct heartbeats, not one. If the dedup key is `(device, timestamp, rr_ms)` it silently collapses them into one beat, which removes a zero-difference interval and biases RMSSD and HRV high at rest and during sleep. The key must include a per-second sequence tiebreaker, giving `(device, timestamp, rr_ms, seq)`. This is the exact fix that landed as a patch in the surveyed lineage, and it is one line in the schema that changes a resting HRV number by a noticeable margin, so it gets its own failing test in `mav-timeline` before the dedup code is written.

The timeline never interpolates and never rewrites a raw device timestamp. When a device clock is implausible or stale, the correction is a stored mapping from device time to wall time, and the raw timestamp survives untouched so the correction can be inspected and reversed. `mav-store` keeps raw evidence in append-only tables; derived tables are rebuildable from raw data plus algorithm versions, and dropping every derived table and recomputing must produce identical values. That round-trip is itself a test.

Capability negotiation ties the analytics to the data. Each analytic declares `requires: [StreamKind]` as data, not as a hardcoded device check. At connect time the engine intersects what a device's manifest says it produces with what each analytic requires, and produces an availability set. The UI reads that set. An analytic that cannot run is visible in the inspector as, for example, "unavailable: missing RR", rather than silently disappearing. Nothing downstream is allowed to hardcode "if device is a WHOOP 4.0".

The engine recomputes on triggers, not on a timer for its own sake: a disconnect finalises the session, a completed historical sync recomputes the affected days, local midnight finalises the day, the UI can ask for an on-demand recompute, and low battery pauses non-essential recomputation. Every computed value is cached under a key of `(content hash of the input stream slice, algorithm version)`, so a value is reused exactly when its inputs and its algorithm have not changed.

Observability runs alongside all of it. The `Tap` trait's `on_stage(&self, stage, event)` is called at every boundary with counts and ids and, in debug builds, payload summaries. The standing requirement is walk-back: for any metric value on screen it must be possible to follow stored ids from the metric back to its features, to the samples, to the frames, to the raw bytes. If you cannot walk a number back to the bytes it came from, the observability is incomplete and that is a bug.

## How the swarm executes work packets

A work packet is the unit of agent work. Milestones are broken into packets in the files under `docs/plans/active/`, and each packet is written so that one agent can pick it up and finish it without asking a question the packet should have answered. The template is fixed:

```
Packet M1-P3: <one-line goal>
Owns:           the exact files this packet creates or may modify
Must not touch: everything else (list explicitly where it could be ambiguous)
Contract:       the exact signatures and types this packet implements or consumes,
                which are frozen and come from mav-model or an earlier packet
Tests first:    the test names to write, and what each one asserts
Exit:           the commands that must pass (cargo test -p ..., tools/check_docs.sh, ...)
Notes:          gotchas and references into docs/
```

The protocol for executing one packet is defined in `skills/work-packet` and runs in a fixed order. Read the packet. Read the docs it references. Write the listed tests and watch them fail, because a test that has never failed has proven nothing. Implement until they pass. Run the full gate set, not just your own crate's tests. Update the packet's status and the decision log in the plan file. Commit. One packet is one commit or one pull request.

Two rules keep the swarm from colliding. An agent never edits a file another packet owns; if two packets seem to need the same file, the packets were cut wrong and that is fixed before work starts. And an interface dispute is resolved by an ADR, never by a local workaround. If the contract you were handed is wrong, you stop and raise it; you do not quietly widen a type or add a special case to make your packet pass, because the next agent is depending on the contract as written.

CI is the mechanical backstop under all of this. Every pull request must pass `cargo fmt --check`, `clippy` with warnings denied, `cargo test --workspace`, `tools/check_docs.sh`, and `tools/check_deps.py`. A red gate blocks the merge. `clippy` also denies `unwrap`, `expect`, and `panic!` in library code (they are allowed in tests), because a swarm that panics on an unexpected byte takes the whole pipeline down instead of logging a reason code and moving on.

When a milestone is finished, its file moves from `docs/plans/active/` to `docs/plans/completed/` with a short retro section: what surprised us, what drifted from the plan, and which doc fixes were filed as a result.

## Milestones

Milestones are gated by exit criteria, not dates. A milestone is done when its exit line is demonstrably true, and not before. Each milestone gets a full packet breakdown in `docs/plans/`; M0, M1, and M2 are broken down now, and M3 through M8 are broken down as they come up.

| # | Name | Scope | Exit criterion |
|---|---|---|---|
| M0 | Bedrock | Workspace, CI, the docs system, `mav-model` frozen, `mav-frame` complete (CRC 8/16/32, reassembler, TypedReader) with property tests, error taxonomy plus tracing plus the ring log, a UniFFI hello-world binding, `tools/check_*` live | CI green; both platforms link the core; the docs checks pass |
| M1 | First vertical slice | Realtime HR from a WHOOP capture, end to end: acquisition state machine, whoop5 manifest (realtime subset), decode the HR packet, minimal SQI, timeline insert, store, an HR feature, a "current HR plus session summary" snapshot, the FFI query, `mav-replay` driving it all from a fixture, and the parity harness running on both platforms | The same capture file produces an identical snapshot hash on the iOS and Android simulators |
| M2 | Mock second device | `connectors/mock` with a deliberately different frame format and a codec that needs per-device state; proves the manifest-plus-codec abstraction with no core edits allowed | The mock streams through the untouched pipeline, and a doc section "what the abstraction survived" is written |
| M3 | RR, HRV, Recovery | RR intervals, HRV, recovery, and capability negotiation end to end, with recovery hidden when RR is absent and that absence proven using the mock device with no RR stream | Recovery computes when RR is present and is correctly reported unavailable when it is not |
| M4 | Sleep | Gravity, HR, RR, and respiration features; staging (rule-based first, an ML stager only behind the admission rule); sleep windows, efficiency, and a night-summary snapshot | A capture produces a night summary with staged sleep and the numbers trace back to samples |
| M5 | Historical sync and canonical merge | The backfill state machine, every known record version from the ledger, clock-correction segments, plausibility gates, and recompute triggers | A historical capture backfills, merges canonically, and recomputes affected days without inventing or dropping data |
| M6 | Analytics breadth and ML | Strain and the remaining metrics that pass the admission rule; Rust preprocessing plus native inference wired with golden vectors; every analytic engine justified in writing or culled | Every shipped metric has a fixture or a published reference, and no engine exists without one |
| M7 | Cloud connector | A manifest of kind `cloud`, an ingest adapter, the same pipeline, proving the connector abstraction covers non-BLE sources too | A cloud source streams through the same pipeline the straps use, with no core edits |
| M8 | Hardening | Observability audit demonstrating the walk-back requirement, the error-report UX, a fixture-coverage audit, doc gardening, a dependency-edge audit, and ADR backfill | Walk-back is demonstrated end to end, coverage gaps are closed or documented, and the docs match the code |

Alongside the numbered milestones there is a standing checklist that is not a milestone because it has no fixed place in the sequence: the hardware epoch. When the straps arrive, every fact currently marked as code-inferred gets verified or corrected against live captures, its ledger tag flips from code-inferred to hardware-verified, and the fixtures that were built from surveyed captures are regenerated from our own real captures. This lives as a running checklist in `docs/protocol/whoop.md`. It is written down now, before the hardware exists, so that the day a strap goes on a wrist the work is a list to tick through rather than a question of where to even start.

## What we are explicitly not building

Some things are out of scope on purpose, and naming them is as useful as naming the scope, because it stops an agent from helpfully building something nobody asked for.

There is no plugin marketplace or store, and no community plugin ecosystem. There is no WASM and no in-app code execution of any kind; connectors are data and a narrow Rust trait, not sandboxed programs, and they get no network, filesystem, or BLE access of their own. ML inference does not run in Rust — it runs natively on CoreML and TFLite, with Rust owning only the deterministic preprocessing. BLE transport does not live in Rust either; the native platforms own the radios, and the Rust core receives bytes through one bounded channel. There are no event buses, which is principle 1 restated as a boundary. There are no speculative algorithms without fixtures, and no analytics knowledge baked into manifests. The timeline does not interpolate. HealthKit, Health Connect, widgets, and notifications are not built until after M8. And the apps stay thin snapshot-rendering shells; there is no UI work beyond that until the platform underneath it is real.

A note honest enough to write down: neither surveyed codebase actually shipped a neural model. Everything they computed was classical DSP and statistics. The native-inference boundary in `docs/ml.md` is kept as architecture for the day a real model with a golden vector exists, but we do not add a CoreML or TFLite dependency until that day comes. Building the runway before there is a plane is exactly the speculative work the admission rule exists to prevent.
