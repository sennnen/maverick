# Architecture

Maverick is one local-first Rust data platform behind two thin native shells. Native iOS and Android
own OS integration: BLE permissions and radio execution, platform lifecycle, file/URL acquisition,
presentation, and any admitted native ML runtime. Rust owns connector installation and execution,
transport policy enforcement, decoding, storage, analytics, diagnostics, and immutable read models.
No health data leaves the phone.

## Target component map

```text
iOS / Android
  acquire .mavconn bytes + source metadata
  execute validated generic BLE actions
  render immutable snapshots
        |
        v
mav-ffi
        |
        v
mav-engine ---- mav-connector-runtime ---- pure WebAssembly interpreter
     |                    |                         |
     |                    | event -> action ABI     | one hostile .mavconn instance
     |                    | limits + trust          | no host capabilities
     v                    v                         v
typed pipeline       installer / rollback      connector-private state
     |
     v
append-only store -> features -> analytics -> snapshots
```

Every acquisition source converges on one Rust API accepting bytes and `ConnectorSource` metadata.
The core verifies and self-tests those bytes before activation. A connector instance receives only
normalized events and returns declarative actions. Core validates actions against the signed
manifest, executes them through the native transport, and returns results as events. The connector
never sees CoreBluetooth, Android BLE, a filesystem, network, database, process, thread, wall clock,
or random source.

The exact artifact, ABI, install, trust, and lifecycle contracts are in
[connectors.md](connectors.md). [ADR-017](adr/ADR-017.md) records the decision. The current bundled
driver and its deletion inventory are in [connector-audit.md](connector-audit.md). WC-P1 through
WC-P5 implement the leaf ABI, non-executing artifact/trust inspection, public SDK/toolchain,
bounded interpreter instance, and normalized host event/action session. Until the later migration
packets land, code still contains the audited ADR-016 compiled path and the new runtime is not yet
exposed over the product FFI transport path.

## Native/Rust line

Native layers eventually do only these connector tasks:

- acquire connector bytes from URL, local file, document picker, share/open flow, or registry;
- pass bytes plus source metadata to core;
- request explicit user approval using the inspection report returned by core;
- perform generic scan, connect, pair, discover, subscribe, read, write, disconnect, and cancellation
  actions that core already validated;
- return normalized results and lifecycle events to core.

Native code never identifies WHOOP commands, parses packets, chooses protocol retries, acknowledges
history, or repairs connector output. iOS can enforce a narrower publisher allowlist than Android
without changing `.mavconn` bytes or ABI.

ML inference remains native only when a real admitted model requires CoreML or TFLite. Rust owns
deterministic preprocessing. This reserved boundary is independent from WebAssembly connectors.

## Synchronous pipeline and async seam

The health-data pipeline remains a synchronous typed call graph. WebAssembly does not introduce an
event bus. Connector calls are synchronous and bounded. Native transport completion is asynchronous
at one host seam; normalized completion events re-enter `mav-engine`, which invokes the connector and
then the pipeline in deterministic order.

For history safety, ordered connector actions are executed serially. An `EmitSamples` action is
committed before a later acknowledgement write can execute. Failure returns an event and stops the
remaining dependent actions. This preserves append-before-ack without encoding WHOOP in core.

## Crates after migration

| crate | responsibility |
|---|---|
| `mav-model` | Frozen health-data and error vocabulary; no device protocol types |
| `mav-frame` | Generic byte/frame primitives usable by host and SDK tests |
| `mav-connector-abi` | Frozen no-device event, action, artifact metadata, and ABI wire types |
| `mav-connector-sdk` | Public guest exports, allocation glue, bounded builders, metadata, and native harness |
| `mav-connector-runtime` | `.mavconn` parser, verifier, interpreter adapter, limits, instance lifecycle |
| `mav-connector-tool` | Deterministic pack, inspect, trust/export validation, and executable fixture CLIs |
| `mav-connector-store` | Install records, trust records, connector-scoped state, activation and rollback transactions |
| `mav-codec` | Normalized sample admission and any open-standard profiles retained by explicit ADR; no loadable-device registry |
| `mav-timeline` | Ordering, deduplication, clock correction, canonical merge |
| `mav-sqi` | Signal quality before normalization |
| `mav-feature` | Primitive, derived, aggregate features with provenance |
| `mav-analytic` | Metric DAG and capability negotiation |
| `mav-store` | Append-only evidence, derived data, provenance, errors |
| `mav-obs` | Tracing, taps, bounded report bundle |
| `mav-engine` | Orchestration, connector event/action loop, pipeline, recompute |
| `mav-ffi` | Platform-neutral installation, connector management, transport events/actions, snapshots |
| `mav-replay` | Same runtime and artifact path without a phone |

Names may change only in the owning packet before public interfaces freeze. No crate under
`core/connectors/` remains after migration.

## Allowed dependency direction

Exact edges are enforced by `tools/check_deps.py`:

```text
mav-model       mav-connector-abi <- mav-connector-sdk
      ^              ^
      |              +--------- mav-connector-tool
stage crates    mav-connector-runtime <-+
      ^              ^
      +------ mav-engine ------+
                  ^             |
              mav-ffi       mav-replay
```

`mav-connector-abi` depends on no Maverick crate. The public SDK depends only on that leaf. The
developer tool depends on ABI plus runtime so its checks cannot drift from the host. Runtime depends
only on the ABI and `mav-model` error vocabulary inside Maverick. Runtime cannot depend on a device,
frontend, analytics, tool, SDK, or native BLE API.
Engine depends on runtime; runtime never calls engine. FFI/replay do not link connector crates.
Connector source lives only in `sennnen/maverick-connectors` and compiles against the public SDK.

## Adding a device after migration

A developer creates a standalone Rust connector project using `mav-connector-sdk`, writes unit and
state-machine tests, embeds bounded golden fixtures, packages and signs one `.mavconn`, validates it
with the same runtime used by the apps, and publishes it directly or through a registry. Maverick
source, Cargo manifests, FFI, iOS, and Android do not change.
