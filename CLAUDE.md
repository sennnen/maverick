# Maverick (mav)

Maverick, or mav in code, crate names, and on the CLI, is a local-first wearable-data
platform for iOS and Android. BLE straps stream to the phone, and every byte of decoding
and analytics runs on-device in a shared Rust core; nothing leaves the device. WHOOP 4.0
and 5.0/MG come first, and because no hardware exists yet, the whole protocol side is built
and tested from captured fixtures until the straps arrive.

This file is a map, not a manual. It goes into every session, so it stays short and points
at the documents that hold the real detail. Read the one you need when you need it.

## The map

```
maverick/
  CLAUDE.md / AGENTS.md   this file, twice; CI fails if the two differ by a byte
  README.md               the human-facing introduction
  docs/
    PLAN.md               master plan: principles, milestone table, work-packet protocol
    architecture.md       crate ownership and the dependency edges allowed to exist
    pipeline.md           the stage-by-stage contracts data moves through
    connectors.md         manifest schema, the DeviceCodec contract, how to add a device
    protocol/whoop.md     every known WHOOP fact, each carrying a confidence tag
    testing.md            fixture rules, property tests, parity, what counts as a real test
    errors.md             the error taxonomy, numeric codes, logging, the report bundle
    storage.md            append-only schema, forward-only migrations, provenance tables
    ml.md                 the native-inference boundary and Rust-side preprocessing
    adr/                  architecture decision records, ADR-001 onward
    plans/active/         the milestone a packet is drawn from; move to completed/ when done
  skills/                 the four workflows below, each a SKILL.md that loads on demand
  core/crates/
    mav-model             the frozen types every other crate speaks in; changes need an ADR
    mav-frame             CRC 8/16/32, the reassembler, the TypedReader
    mav-codec             the DeviceCodec trait, manifest types, the device registry
    mav-timeline          ordering, dedup, clock correction, historical merge
    mav-sqi               signal quality, scored on raw signals before normalization
    mav-feature           primitive, derived, and aggregate features
    mav-analytic          the metric graph and capability negotiation
    mav-store             rusqlite storage, append-only, provenance, the error journal
    mav-obs               tracing setup, the Tap trait, the report bundle
    mav-engine            orchestration: triggers, task graph, caching
    mav-ffi               the uniffi facade the apps bind to
    mav-replay            runs a capture file through the whole pipeline; our stand-in for hardware
  connectors/
    whoop4, whoop5        a manifest.json each, plus a codec crate only where logic needs one
    mock                  a deliberately odd fake device that keeps the abstraction honest
  fixtures/               golden fixtures, versioned; see fixtures/README for the naming rules
  apps/ios, apps/android  thin binding shells; the real app work is a later milestone
  tools/                  check_docs.sh and check_deps.py, the mechanical gates
```

## How to work

Read docs/PLAN.md before any structural work. It holds the principles and the milestone plan,
and skipping it is how a packet ends up fighting a decision that was already made.

Work comes in packets. A packet lives in docs/plans/active/M*.md and names the files it owns,
the tests to write first, and the commands that must pass. Load skills/work-packet and follow
it; that is the contract for one unit of agent work.

Before a commit, every one of these has to pass:

    cargo test --workspace
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    tools/check_docs.sh          links resolve, CLAUDE.md == AGENTS.md, plans indexed
    tools/check_deps.py          crate dependency edges match architecture.md

Skills, loaded when the task calls for them:

- `skills/work-packet`: executing a packet from docs/plans/active/. Load it before you start one.
- `skills/golden-fixtures`: creating or regenerating a fixture. Load it before you touch fixtures/.
- `skills/connector-authoring`: adding a device. Load it before writing a manifest or a codec.
- `skills/doc-gardening`: the periodic sweep for docs that have drifted from the code.

## Rules that always apply

- No unwrap, expect, or panic in library code; clippy denies them. Tests may use them.
- Every test asserts a computed value or an exact error. A test that cannot fail is a defect.
- Never drop a packet, sample, or frame silently. Log it with an error code and a reason.
- Comments are rare and state a constraint the code cannot show. Narrative knowledge lives in docs/.
- Never hand-edit a fixture to make a test pass. Regenerate it through skills/golden-fixtures.
- Any change to mav-model is a frozen-interface change and needs an ADR first.
- A feature ships on both iOS and Android or on neither. The core is shared, so parity is the default.
- Commits use an imperative subject; put the why in the body when it is not obvious from the diff.
- CLAUDE.md and AGENTS.md are the same bytes. Edit both, or CI will stop you.
