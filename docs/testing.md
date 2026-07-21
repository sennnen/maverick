# Testing policy

The rules in this document are hard rules, and CI enforces them wherever a script can. They exist
because the two codebases Maverick learned from were full of tests that passed and proved nothing,
and a swarm of agents will reproduce that failure mode at scale unless the policy makes it
impossible. An agent under pressure to go green will write a green test; the policy's job is to
make sure green means something.

## A test that cannot fail is a defect

This is the first rule and the one everything else follows from. A test exists to be able to fail
when the code is wrong. A test that passes regardless of the code is not a weak test; it is a
defect in the test suite, and it is worse than no test, because it manufactures confidence.

Banned outright:

- `assert!(true)` and its relatives.
- Tests with no assertions. A test must assert computed values or exact errors.
- Snapshot tests that auto-accept their own updates. A snapshot that rewrites itself when the
  output changes cannot fail, by construction.
- Mocking the unit under test. A test that replaces the thing it claims to test is testing the
  mock.

One of the surveyed repos had a 6030-line "validation suite" binary that mostly re-serialised
manifests: large, green, and low-evidence. Do not confuse a big passing harness with coverage. The
question to ask of any test is what change to the code would make it fail, and if the answer takes
more than a moment, the test is suspect.

## Golden fixtures at every boundary

Every pipeline boundary has golden fixtures: input bytes and expected JSON output, at the frame,
sample, timeline, feature, and metric level. A fixture is real evidence, derived from a capture or
a published reference, and the pipeline is held to reproducing it exactly.

Fixtures are versioned, and the versioning rule matters. A fixture states the algorithm versions it
was produced with. When an algorithm version changes, the change requires a **new** fixture file;
the old one is never edited. Editing a fixture to match new output is the snapshot-auto-accept
failure with extra steps, and it destroys the fixture's value as evidence of what the old behaviour
was. The workflow lives in `fixtures/README` and the `golden-fixtures` skill; fixtures are never
edited by hand.

## Property tests

Property tests (proptest) are mandatory for the components where a hand-picked example proves
little because the input space is hostile: the reassembler, the CRC implementations, the
`TypedReader`, timeline dedup, and clock correction. These are the components that face arbitrary
bytes or adversarial orderings, and each has invariants a property test can genuinely exercise: the
reassembler must resynchronise after any garbage prefix, dedup must be idempotent under re-delivery
in any order, a clock correction must never mutate a raw timestamp.

Fuzz targets (cargo-fuzz) run against the frame parsers as a nightly job. Fuzzing is not a merge
gate, but a fuzz finding is a bug like any other.

## The parity harness

`mav-replay` validates one signed artifact and executes its embedded event/action suite through the
same interpreter used by `MavRuntime`. Rust, Swift, and Kotlin feed identical normalized transport
events and must observe identical lifecycle reports, ordered actions, emitted samples, trace hashes,
state hashes, fuel, and linear-memory ceilings. Any platform divergence is a binding or transport-
adapter bug; protocol code is the same artifact bytes.

Every host schema gets a Rust canonical fixture plus strict Swift and Kotlin decode tests. Platform
decoders reject unknown schema names and missing required fields, while ignoring unknown additive
fields inside a known schema. This is compatibility testing, not permission to make fields optional
without evidence.

Runtime-loaded connectors add a target-parity layer. Native author tests execute every embedded
event/action case, and `mavconn-test --report` executes the signed Wasm bytes to freeze canonical
input, ordered action, emitted-sample, final-state, fuel, and linear-memory results. Maverick
regenerates those reports from committed artifact fixtures, while Swift and Kotlin consume the same
schema and ceilings. The exact P11 evidence and its consistency-only limitation live in
[connector-parity.md](connector-parity.md).

## Consistent is not validated

Here is the sharp lesson from the prior codebases, and it needs stating bluntly because it is the
subtlest form of the test-that-cannot-fail.

One of the surveyed repos carried around forty speculative analytics engines whose "golden" tests
proved only that the Swift output equalled the Kotlin output equalled a stored reference generated
by the same code. Those tests were self-consistency tests. They could catch a platform diverging,
and that has value, but they could not catch a formula that was wrong on both platforms, because a
formula that is consistently wrong passes a consistency test forever.

So the policy draws the distinction explicitly. A parity or self-consistency test is necessary but
not sufficient. A metric counts as **validated** only when it has been checked against a
ground-truth measurement or a published reference implementation. Anything short of that is merely
**consistent**, and a consistent-but-unvalidated metric must be labelled provisional wherever it
surfaces. This distinction is the core justification for the algorithm admission rule: an algorithm
lands in Maverick only when it has a golden fixture derived from a real capture or a published
reference, or property tests that can genuinely fail. Anything else stays an explicit stub that
capability negotiation reports as unavailable.

## The round-trip rule

Derived data is disposable, and there is a test that keeps it so: drop every derived table,
recompute from the raw tables and the recorded versions, and the recomputed values must be
identical to what was dropped. If they are not, something derived was not derivable, which means
either a computation is nondeterministic or some state leaked into a table it should not be in.
Either is a real bug. The storage side of this guarantee is described in [storage.md](storage.md).

## Testing without a radio

Neither prior codebase could CI-test its real BLE state machine, because the state machine was
entangled with CoreBluetooth or the Android stack and needed hardware to drive it. Maverick's
acquisition state machine is pure Rust, driven by injected events, and fed by capture replay
through `mav-replay`. It is therefore unit-testable with no radio at all: a capture file exercises
the same code the live device would, transitions and retries included. The only part that cannot be
tested until the straps arrive is the thin native transport shim, and that is a deliberately small
surface.

## CI gates

Every pull request must pass, and merge is blocked on red:

```
cargo fmt --check
cargo clippy -- -D warnings
cargo test --workspace
tools/check_docs.sh
tools/check_deps.py
```

The clippy configuration denies `unwrap`, `expect`, and `panic!` in library code (they are allowed
in tests). `check_docs.sh` verifies that links resolve, that `CLAUDE.md` and `AGENTS.md` are
identical, and that plan files are indexed. `check_deps.py` verifies the crate dependency graph
matches the allowed edges in [architecture.md](architecture.md).
