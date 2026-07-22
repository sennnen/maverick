# The pipeline

Data moves through Maverick as a straight sequence of typed stages. Native transport results enter
the connector host, and by the end of the sequence there is an immutable snapshot the UI can read.
Between those points the data passes through ten stages, in order. This document gives the contract
for each stage: what it takes, what it returns, and what it is and is not allowed to do.

## Why a call graph and not a bus

The plan Maverick grew out of had six publish/subscribe buses. That design was rejected before any
code was written. A bus hides call order. When a stage publishes an event and some other stage
happens to be subscribed, the sequence in which things run is a property of the runtime and the
subscription list, not of anything you can read on the page. Replaying a capture through a bus does
not deterministically reproduce the same order, which makes debugging a matter of luck. Worse for
this project, pub/sub is where a swarm of agents produces spaghetti fastest: each agent adds a
subscriber, no agent owns the flow, and within a few milestones nobody can say what runs when.

The pipeline is a synchronous typed call graph instead. Every stage has the same shape:

```rust
fn run(input: StageIn, ctx: &mut StageCtx) -> Result<StageOut, MavError>
```

The stage boundaries `StageIn` and `StageOut` are frozen types in `mav-model`. Because the graph is
synchronous and the types are fixed, the order of execution is exactly the order written in
`mav-engine`, and feeding the same normalized events in twice produces the same work in the same
order. That is what makes `mav-replay` a real tool rather than an approximation: it executes the
same signed artifact bytes, event/action ABI, limits, and sample-admission path as a live session.

Inspectability, the one genuine thing a bus gives you, comes from the `Tap` trait instead. The
engine invokes the tap at every stage boundary with counts, ids, and (in debug builds) a summary of
the payload. A tap is a passive observer; it cannot change the data or the flow, only watch it. The
default taps are a set of metric counters and the ring-buffer log. Because the tap fires at every
boundary, you get the same visibility a bus subscriber would have given, without giving up a
deterministic call order. Observability is described in full in [errors.md](errors.md).

The single asynchronous seam is native transport completion. The host drains bounded declarative
actions, native code executes them, and normalized completion events re-enter the synchronous
connector host. Nothing downstream of that re-entry is async.

## StageCtx

Every stage is handed a `&mut StageCtx`. It is the stage's only door to the outside world, and it
is deliberately narrow. Through it a stage can emit tap events and log errors against the current
ids. Connector state is not a stage handle: signed artifacts request bounded transactional state
actions from the host. The context carries the current `DeviceId`, `SessionId`, and the ids of the frame
or stream in flight, so that anything logged is traceable back to the bytes that caused it. A stage
does not receive a handle to another stage. The only way one stage's output reaches another is by
being returned up to the engine and passed down as the next stage's input.

## The stages

### 1. Acquisition

**In:** a host-validated `ConnectorTransportAction`.
**Out:** one normalized `ConnectorEvent`, including its host wall time when applicable.

Swift and Kotlin execute only generic scan, connect, pair, discover, subscribe, read, write,
disconnect, and timer requests. Core assigns operation and deadline ids, bounds queues and resource
use, correlates each result, rejects late generations, and never exposes a native handle to guest
code. Native acquisition **may** use platform Bluetooth and lifecycle APIs. It **may not** parse a
device packet, choose a retry, acknowledge history, or synthesize connector event order.

### 2. Connector execution

**In:** a normalized event for one verified connector session.
**Out:** a bounded, ordered batch of declarative actions.

The no-JIT runtime invokes one signed `.mavconn` instance. Framing, CRC checks, authentication,
commands, retries, historical cursors, and device-specific state live inside that artifact. The
host validates every action against signed permissions and declarations before execution. Sample
actions commit before any later acknowledgement write becomes visible; cancellation advances a
generation and invalidates queued or late results. A rejected byte, frame, or state transition must
produce a bounded diagnostic or typed host failure rather than disappear.

### 3. Sample admission

**In:** validated `EmitSamples` output from a signed artifact, or an explicitly admitted Bluetooth
SIG profile value.
**Out:** `RawSampleBatch` — a batch of raw samples, one stream kind at a time, with device
timestamps and raw (un-normalised, un-scored) values.

Device-specific reassembly, decode, protocol state, and learned state execute inside a sandboxed
`.mavconn`. Reassembly is the connector's own, one buffer per notify characteristic, because a
notification boundary is an MTU artefact and a frame boundary is device knowledge (ADR-020); a
partial frame never survives a reconnect. `ConnectorHost` validates bounded `EmitSamples` against
signed stream and unit
declarations, converts them into `RawSampleBatch`, then sends them through SQI, timeline,
provenance, and storage. Contract: [connectors.md](connectors.md).

Admission **may** read signed declarations and produce raw samples. It **may not**
score signal quality, correct clocks, normalise units into calibrated physical values that hide the
raw reading, or write to storage. A raw sample carries the number the device sent and the device's
own timestamp, nothing yet interpreted about its trustworthiness.

### 4. SQI

**In:** `RawSampleBatch`.
**Out:** `Sample<T>` with a `Quality { score, reason }` attached.

Signal quality scores raw signals before any normalisation, so that the quality judgement is made
against what the sensor actually reported rather than against a cleaned-up derivative. Each sample
comes out with its value, a quality score in `0.0..=1.0`, and an optional reject reason. A low score
does not delete the sample; it travels with it, so that a later stage can decide what a poor-quality
reading is worth for a given metric. SQI **may** attach scores and reasons. It **may not** drop a
sample; a sample scored zero is still a sample, and dropping it silently would violate the
no-silent-drops rule below.

Three groups of stream are scored differently, and the distinction is load-bearing. A **measured
signal** — heart rate, RR — is gated on physiological plausibility. An **exact-on-wire readout** —
a battery percentage, a wrist flag, a step counter, a device-stated code — is a number the strap
asserted rather than one we inferred, so it carries `Quality::exact()` once its value is inside the
range the reading can occupy; a value outside that range is rejected with a reason, because a
battery cannot be at 140%. A **raw count** nobody has judged yet stays `Quality::unassessed`, which
scores zero.

That third default is the safe one, and it is why the boundary matters: the FFI drops samples
scoring zero or below, so a readout left unassessed is a readout the app can never see, however
faithfully a connector emitted it.

### 5. Timeline

**In:** scored `Sample<T>` values, from realtime decode and from historical sync.
**Out:** an ordered, deduplicated, clock-corrected series of `Sample<T>`.

The timeline is where realtime and historical data become one canonical series. It orders samples,
removes duplicates, corrects clocks, and merges backfilled history with data already seen. It is
governed by two hard rules that hold everywhere in Maverick and are stated again below: it never
interpolates, and it never mutates a raw timestamp.

#### Placement

A plausible device timestamp is trusted directly. An implausible one is placed through the session's
`ClockMap`, which shifts a whole segment by one learned offset and so preserves the gaps between
samples; only when no correction covers the sample does the capture instant apply (ADR-022). All
three cases are distinguishable, and the second and third carry
`RejectReason::ImplausibleTimestamp`, because neither came from a clock worth trusting.

The distinction matters because the fallback is destructive: a burst of samples ten seconds apart in
device time, all placed at one capture instant, has had every interval inside it set to zero — and
those intervals are what variability analysis reads.

#### Commit accounting

One `EmitSamples` batch produces three numbers, and conflating them is how a sample disappears
without anyone noticing. **Emitted** is what the connector handed over. **Persisted** is what
reached storage. **Duplicate** is what the timeline or the store recognised as already held.

The ABI acknowledgement is the *emitted* count. Acknowledgement means received and safely handled,
and a sample the pipeline already holds has been safely handled — a connector that had to re-emit
whatever it was not acknowledged for would replay history forever. Duplicates are therefore normal,
particularly during a backfill, but never silent: a commit that drops any is journalled under
`CONNECTOR_HOST_SAMPLE_DUPLICATE`, and the session totals are on the lifecycle snapshot. A sample
that is neither persisted nor counted as a duplicate has been lost, and the arithmetic says so.

Provenance is written only for samples that actually persist. A provenance row for a deduplicated
sample points at nothing and makes walk-back lie about what produced a value.

Deduplication itself is two layers (ADR-021). The timeline's key set is a bounded window — the fast
path — because one Timeline lives for a whole session and a multi-day backfill would otherwise hold
a key for every sample ever banked. The store's natural key is the durable layer and the one that
must be correct. A duplicate older than the window passes the timeline and stops at the store, which
is why the store's insert outcome feeds the accounting above rather than being discarded.

Deduplication carries an invariant that was learned the hard way in the prior codebases and is
easy to get wrong. Two equal RR intervals in the same second are two distinct heartbeats, not one
beat counted twice. If the dedup key is `(device, ts, rr_ms)`, the second of two equal intervals in
one second collapses into the first, a zero-difference beat vanishes, and RMSSD (and every HRV
figure built on it) biases high. The key must therefore include an in-second sequence tiebreaker:
`(device, ts, rr_ms, seq)`. This is not a nicety. It is a proven historical failure mode, and in
Maverick it is an invariant test in `mav-timeline`, not a comment.

Clock correction is done by storing a mapping, never by editing the sample. When a device's RTC is
implausible (outside a sane unix window) the timeline records a `ClockMap` segment that translates
device time to wall time, and flags the affected samples; the raw `device_time` on the sample is
left exactly as the device sent it. The rationale and the plausibility windows are in
[protocol/whoop.md](protocol/whoop.md); the storage of the mapping is in [storage.md](storage.md).

### 6. Store

**In:** the canonical `Sample<T>` series.
**Out:** storage receipts (the ids under which the samples were written).

Store writes samples to the append-only raw tables. Re-syncing the same history is idempotent:
inserting a sample that is already present is ignored rather than duplicated or overwritten. Store
**may** append and dedupe on insert. It **may not** alter a sample's value on the way in. The full
storage model, including the three tiers and the round-trip guarantee, is in
[storage.md](storage.md).

### 7. Features

**In:** stored `Sample<T>` slices.
**Out:** `Feature` values, each carrying a `MetadataId` that points at its provenance.

Features come in three tiers. Primitive features are computed directly from samples; derived
features are computed from primitives; aggregate features summarise a window. Every feature value
records where it came from through its `MetadataId`, which links to a provenance row naming the
source stream, the quality, the algorithm id, the algorithm version, and the sample count that went
into it. That link is what makes the walk-back requirement possible: from a number on screen you can
trace feature to samples to frames to raw bytes. Features **may** read stored samples and other
features they depend on. They **may not** invent inputs; a feature whose required stream is absent
is not computed, and its absence is visible through capability negotiation rather than filled in.

### 8. Predictions

**In:** feature slices, preprocessed in Rust.
**Out:** `Prediction { value, confidence }`, entering the pipeline as first-class features.

Predictions are where a model, if there is one, runs. Rust does the preprocessing (resample, filter,
FFT, spectrogram) and hands a tensor to the native inference runtime; the native side returns a
prediction and a confidence, and that result re-enters the pipeline as a feature with provenance
like any other. Today there is no model, so this stage is a defined boundary with no inference
behind it; see [ml.md](ml.md). Predictions **may** call the native inference shim through the
engine. They **may not** run inference in Rust, and they **may not** enter the pipeline without the
same provenance every other feature carries.

### 9. Metrics

**In:** features and predictions.
**Out:** `Metric` values (recovery, strain, sleep quality, and the rest).

Metrics are the analytics the user actually sees, computed over features by the metric DAG in
`mav-analytic`. A metric is admitted into the codebase only under the admission rule (a golden
fixture from a real capture or a published reference, or property tests that can genuinely fail);
anything without that stays a stub that capability negotiation reports as unavailable. See
[testing.md](testing.md) for the rule and [ml.md](ml.md) for why a metric that only agrees with
itself across platforms is labelled provisional rather than validated. Metrics **may** read features
and other metrics. They **may not** read raw frames or bytes; a metric that needs something the
feature tier does not expose is a sign the feature tier is missing a feature, not licence to reach
past it.

### 10. Snapshots

**In:** metrics and the features behind them.
**Out:** an immutable `Snapshot`, the read model the UI queries over FFI.

A snapshot is a frozen view assembled for the UI. The apps read snapshots and render them; they do
not compute over live pipeline state. This keeps the UI thin and keeps every value it shows
traceable to the ids the snapshot was built from.

## Capability negotiation

Not every device produces every stream, and an analytic that needs a stream the device does not
produce should be visibly unavailable rather than quietly missing. Each analytic declares the
stream kinds it requires as data, for example `requires: [RrInterval]`. When a device connects, the
engine intersects the stream kinds the device's manifest says it produces with the requirements
each analytic declared, and produces an availability set. The UI reads that set. Nothing downstream
hardcodes a device check.

The consequence is that a recovery metric on a strap with no RR data shows up in the inspector as
"unavailable: missing RR", which is a true and legible statement, rather than being absent with no
explanation or, worse, computed from nothing. Capability negotiation tests this directly with
declared stream sets: no fake device family is needed to prove that missing RR makes an analytic
unavailable.

## Hard rules

These hold at every stage and are the ones most likely to be violated by a well-meaning shortcut.

**The timeline never interpolates.** If there is no sample at a time, there is no sample at that
time. Maverick does not manufacture a reading to fill a gap, because a manufactured reading is
indistinguishable downstream from a measured one and corrupts every metric computed over it.

**The timeline never mutates a raw timestamp.** A clock that is wrong is corrected by storing a
mapping from device time to wall time and flagging the sample, and the raw `device_time` is
preserved as sent. A re-sync must be able to reproduce the same correction from the same raw input.

**Nothing is dropped silently.** Every discarded byte, fragment, frame, or sample is logged with an
error code and a reason. A frame that fails CRC, a fragment that cannot resynchronise, a sample
outside a plausibility range: each leaves a record. A swarm that is allowed to drop data quietly
ships silent corruption, and the whole observability model in [errors.md](errors.md) exists to make
sure it cannot.
