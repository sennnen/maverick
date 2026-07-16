# Architecture

Maverick is a wearable-data platform that runs on iOS and Android and keeps everything on the
phone. A BLE strap streams bytes, the phone decodes and analyses them, and no health data leaves
the device. This document describes how the code is arranged to make that work, and why it is
arranged that way rather than some other way.

The shape of the system follows from one hard fact about how it is built: Maverick is written by a
swarm of AI coding agents steered by a single human. Most of the structural decisions here exist
to keep that swarm from quietly wrecking the codebase. Boundaries are strict, interfaces are
frozen, and the rules that matter are checked by a script rather than trusted to good behaviour.

## One core, two thin shells

The whole of the decoding and analytics logic lives in one Rust workspace, the core. Each platform
gets a thin native shell that owns only the things a shared library cannot own: the BLE radio and
the ML inference runtime. Everything between those two ends is Rust, compiled once and called from
both platforms through UniFFI bindings.

We did not arrive at this by taste. Two earlier codebases in this lineage were surveyed before
Maverick was started, and both had reimplemented the WHOOP decode path twice, once in Swift and
once in Kotlin. Both then had parity bugs where the two implementations disagreed. In one, the
Android reassembler only handled the gen4 frame format and silently mishandled gen5; the iOS side
was correct, so the same strap produced different data on the two phones. That is not a bug you fix
once. It is a category of bug that reappears every time a decode detail is touched on one platform
and not the other, and it is nearly invisible because each platform looks internally consistent.

A single shared core removes that whole category by construction. There is only one reassembler,
one CRC implementation, one timeline. If it is wrong, it is wrong identically on both platforms,
which is a far easier problem than two subtly different wrongs. What remains is the risk that the
thin bindings themselves diverge, and that is caught mechanically: the parity harness runs the same
fixtures through the core on both platforms and compares a hash of the canonical output. Any
difference in that hash is, by definition, a binding bug, because the core that produced it is
byte-for-byte the same library. See [testing.md](testing.md) for how the harness is wired.

## Where the native/Rust line sits

Two things stay native, and the reasons are practical rather than architectural.

The BLE radio stays native because CoreBluetooth and Android's BLE stack are the only sanctioned
way to scan, connect, bond, subscribe to notifications, and write to a characteristic on each
platform, and because the OS-level bond (SMP pairing) is what actually gates access to a WHOOP
strap's command characteristic. There is no portable Rust BLE stack we would trust with that, and
putting the radio in Rust would buy nothing. The native shell owns the connection and hands raw
bytes across one boundary.

ML inference stays native because CoreML on iOS and TFLite on Android are the accelerated runtimes
each platform ships, and a model compiled for one is not portable to the other. Rust owns the
preprocessing that feeds the model (resampling, filtering, FFT, spectrograms, feature extraction),
which is pure and deterministic and can be golden-vector tested; the native side owns only the
tensor-in, tensor-out call. This boundary is described in [ml.md](ml.md). It is worth saying early
that neither prior codebase actually shipped a neural model, and neither does Maverick today; the
native-inference boundary is architecture held in reserve for when a real model with a golden
vector exists, and until then it carries no CoreML or TFLite dependency at all.

Everything else is Rust: reassembly, CRC checking, frame decode, signal quality, the timeline,
storage, features, the ML preprocessing, metrics, and the immutable snapshots the UI reads. The
apps under `apps/ios` and `apps/android` are native product shells. Their visual and interaction
specification comes from the existing Aura screens in the prior NOOP workspace, but only the
presentation layer crosses: design tokens, reusable visual components, the four-hub navigation,
settings placement, and screen composition. NOOP's repositories, BLE clients, analytics, storage,
ML wrappers, and platform view models do not cross. Maverick replaces those with one small native
presentation store fed by immutable core read models. The exact migration and release sequence is
the [platform lane](plans/active/platform.md), and the binding contract is
[platform.md](platform.md).

A native shell may format units, dates, localized strings, and accessibility copy. It may not
compute a health metric, infer why one is unavailable, query the core database directly, or repair
missing data. Every field presented to a screen is explicitly a value, collecting, unavailable, or
failed. That makes an unfinished analytic a truthful UI state instead of an excuse for a platform
fallback.

There is exactly one place where the system is asynchronous, and it is the seam between the native
radio and the core. The native BLE layer pushes received bytes into a bounded channel, and the
pipeline entry reads from that channel. Everything downstream of the channel is a synchronous typed
call graph, for the reasons set out in [pipeline.md](pipeline.md).

## The crates

The core is a Cargo workspace of twelve crates. Each has one responsibility, and the split exists
so that a work packet can own a crate (or a file within one) without reaching into another agent's
territory.

| crate          | responsibility |
|----------------|----------------|
| `mav-model`    | The frozen vocabulary: ids (`DeviceId`, `SessionId`, `StreamId`, `FrameId`, `MetadataId`), time types (`DeviceTime`, `WallTime`, `ClockMap`), the `StreamKind` enum, `Sample<T>`, `Quality`, `MavError`, and the version/`ALGORITHM_VERSION` conventions. Nothing but types and their serde derivations. |
| `mav-frame`    | Transport primitives: the three CRCs (CRC-8, CRC-16/Modbus, CRC-32/zlib), the byte reassembler, and the `TypedReader` that reads little-endian fields out of a validated payload. |
| `mav-codec`    | The `DeviceCodec` trait, the manifest types, and the device registry. This is the connector contract described in [connectors.md](connectors.md). |
| `mav-timeline` | Ordering, deduplication, clock correction, and the merge of realtime and historical data into one canonical series. It never interpolates and never mutates a raw timestamp. |
| `mav-sqi`      | Signal quality. Scores raw signals before normalisation and attaches a value, a quality score, and a reason. |
| `mav-feature`  | Features, in three tiers: primitive, derived, aggregate. Each carries a provenance id. |
| `mav-analytic` | The metric DAG (recovery, strain, sleep quality, and so on) and capability negotiation. |
| `mav-store`    | The SQLite layer over rusqlite: append-only raw tables, regenerable decoded tables, recomputed metrics, the provenance table, the per-device key-value table, and the error journal. |
| `mav-obs`      | Observability: tracing setup, the `Tap` trait, the in-memory ring buffer, and the user-facing report bundle. |
| `mav-engine`   | Orchestration. It owns the triggers (disconnect, historical-sync-complete, local midnight, on-demand, low-battery), the task graph, and the recompute cache keyed by content hash and algorithm version. It is the only crate that wires the stages together. |
| `mav-ffi`      | The UniFFI facade. The one crate the native shells link against. |
| `mav-replay`   | A binary, not a library. It feeds a capture file (hex lines or a btsnoop subset) through the full pipeline and dumps every stage boundary to JSON. It is the main debugging tool and the substitute for hardware until the straps arrive. |

## Allowed dependency edges

The crates are layered, and the layering is enforced. `tools/check_deps.py` reads the actual
dependency graph from the Cargo manifests and fails CI if any edge exists that is not listed below.
The point is not neatness for its own sake. A swarm left to add dependencies freely will eventually
draw an edge that turns the graph into mud, and once mav-analytic can reach into mav-frame there is
no boundary left to reason about. The allowed edges are:

| crate          | may depend on |
|----------------|---------------|
| `mav-model`    | (nothing internal) |
| `mav-frame`    | `mav-model` |
| `mav-store`    | `mav-model` |
| `mav-obs`      | `mav-model`, `mav-store` |
| `mav-codec`    | `mav-model`, `mav-frame` |
| `mav-timeline` | `mav-model` |
| `mav-sqi`      | `mav-model` |
| `mav-feature`  | `mav-model` |
| `mav-analytic` | `mav-model`, `mav-feature` |
| `mav-engine`   | `mav-model`, `mav-frame`, `mav-codec`, `mav-timeline`, `mav-sqi`, `mav-feature`, `mav-analytic`, `mav-store`, `mav-obs` |
| `mav-ffi`      | `mav-model`, `mav-obs`, `mav-engine` |
| `mav-replay`   | `mav-model`, `mav-obs`, `mav-engine` |

A few of these edges are worth a sentence. `mav-model` sits at the bottom and depends on nothing
internal, which is what lets it be frozen: a change to `mav-model` ripples through everything, so
after milestone M0 such a change requires an ADR. The stage crates (`mav-codec`, `mav-timeline`,
`mav-sqi`, `mav-feature`, `mav-analytic`) never depend on each other; they exchange values only
through the frozen types in `mav-model`, and it is `mav-engine` that calls them in sequence. That
is why `mav-engine` is allowed to depend on almost everything while nothing but the two edge crates
is allowed to depend on it. `mav-obs` may read from `mav-store` because the report bundle needs a
slice of the error journal, and the journal table lives in the store. `mav-ffi` and `mav-replay`
are the only two crates that see the assembled pipeline, one for the apps and one for offline
replay, and they are deliberately kept from depending on the individual stage crates so that the
only path into the pipeline is through the engine.

If a packet genuinely needs an edge that is not on this list, that is an interface dispute, and it
is resolved by writing an ADR that adds the edge and explains it, not by adding the edge quietly
and hoping check_deps stays green.

## How a device is added, from up here

The connector story has its own document, but the architectural claim belongs here: adding a new
strap is one `manifest.json` under `connectors/`, plus at most one small codec crate for the logic
that static data cannot express, and zero edits to the core. The manifest holds the static facts
(GATT UUIDs, frame parameters, packet map, field layouts, unit conversions, record versions). The
codec holds only the stateful or learned parts, and it is boxed in: it sees bytes, its own
manifest, and a per-device key-value store, and it cannot touch storage, the network, analytics, or
any other device. When that boundary holds, a new device cannot reach the parts of the system that
would let a decode bug become a corruption bug. ADR-012's custom-frame tests challenge the boundary
with a shape unlike WHOOP without creating a fake device family.
