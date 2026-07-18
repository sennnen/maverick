# Runtime-loaded connectors

This document freezes target contracts selected by [ADR-017](adr/ADR-017.md). It is implementation
architecture, not a claim that current code already implements the runtime. The ordered work lives
in [plans/active/wasm-connectors.md](plans/active/wasm-connectors.md).

## Product invariants

- One `.mavconn` file is both the import unit and a valid WebAssembly module.
- Identical bytes run through identical Rust runtime code on iOS, Android, replay, and tests.
- Adding, updating, rolling back, or removing a connector never rebuilds Maverick.
- Metadata, compatibility, trust, signatures, and embedded tests are checked before activation.
- Connector code is hostile: no ambient capabilities, bounded resources, typed actions only.
- No device family is named by generic core or frontend code.
- Frontends acquire bytes; core validates, verifies, tests, installs, and activates them.

## Trust boundaries

Untrusted inputs are connector bytes, custom-section payloads, Wasm bytecode, imported source
metadata, registry responses, connector state, device advertisements, BLE values, and every action a
connector emits. Parser success never establishes trust. Validation order is:

1. size and structural limits;
2. Wasm magic/version and section scan without instantiation;
3. required/duplicate/unknown-critical custom-section rules;
4. deterministic CBOR and schema validation;
5. ABI/core compatibility;
6. artifact digest and Ed25519 signature;
7. publisher policy and revocation;
8. Wasm validation, import/export allowlist, feature limits;
9. ephemeral instantiation under hard limits;
10. embedded fixture self-tests;
11. explicit user approval and atomic install.

Failure leaves active artifact and state unchanged. Diagnostics contain safe ids/hashes, never raw
health payloads or local file paths by default.

## `.mavconn` format v1

The file starts with normal WebAssembly magic/version bytes. The packer emits required custom
sections after standard Wasm sections in this exact order:

1. `mav:manifest`
2. `mav:abi`
3. `mav:fixtures`
4. `mav:signature`

Each appears exactly once. Any duplicate `mav:*` section, malformed length, trailing bytes, or
unknown `mav:critical:*` section rejects the artifact. Non-Maverick custom sections are allowed but
count toward artifact limits and signature bytes. Custom sections are valid metadata containers
under the [WebAssembly core specification](https://webassembly.github.io/spec/core/appendix/custom).

Payloads use RFC 8949 deterministic CBOR: definite lengths, shortest integer encoding, canonical map
key order, no duplicate keys, no indefinite items, and no floats in manifest/ABI/signature payloads.
The validator rejects a semantically valid but non-deterministic encoding. Fixture numeric values use
explicit integer/fixed-point or byte encodings defined by their schema.

ABI v1 uses ascending unsigned integer map keys and append-only numeric enum indexes. Canonical
decoding re-encodes the typed value and requires byte equality, which also rejects unknown or
duplicate fields. The frozen CDDL SHA-256 hashes are
`b901e5a701e7af5794b74ff5beb05512a1e6fa0e3e76cc7c97dc72f8b66d2ea8` for ABI events and actions,
`4ebeb126d4c17eeaccdab69320cb6d085d3b060a3d413c1e3bc8c8362ec7912b` for manifests,
`1daaa3a4ea07e1c130461c61fc9a0e0d8433db60ac56f8b9bbc1073ba9cbf1ff` for fixtures, and
`be8508dcc5fb1089828ddb7beb9fdcd5303dfaa8a95bf5c4c52f21cd5751587e` for signatures. Hash bytes are
public constants in `mav-connector-abi` and mechanically checked against the schema sources.

WC-P2 inspection is parse-only: pinned `wasmparser` 0.253.0 validates one core module and exposes
bounded custom sections without instantiation. SHA-256 0.11.0 hashes the full artifact, fixture set,
and domain-separated canonical unsigned bytes. Pinned `ed25519-dalek` 3.0.0 performs strict Ed25519
verification; `subtle` 2.6.1 compares signed digests in constant time. Artifact, section-count,
section-size, key-validity, publisher-scope, rotation, revocation, and stale-policy failures close
with Connector-category error codes. Signing keys exist only in test/tool code; runtime stores public
verification keys only.

### `mav:manifest`

Required fields:

```text
schema: "mavconn-manifest/v1"
connector_id: reverse-DNS stable id
version: SemVer without build metadata
display_name, description
publisher_key_id
abi: { major, min_minor, max_minor }
core: { min_version, max_version? }
state_schema: u32
artifact_limits_profile: named v1 profile
device_families: [identity + advertisement match rules]
services: [service + characteristic declarations]
capabilities: [declared stream kinds and transport operations]
permissions: [closed enum; BLE-only in v1]
entrypoints: fixed v1 export names
fixture_set_hash
update: { channel, downgrade_policy }
```

Advertisement rules can match declared service UUIDs, manufacturer id plus bounded masked bytes,
and normalized name prefixes. Regex, arbitrary code, and hidden registry lookup are forbidden.
Characteristics declare logical ids, UUIDs, properties, sensitivity, and whether confirmed writes
are required. Actions may reference only logical ids declared here.

### `mav:abi`

Contains `schema: "mavconn-abi/v1"`, ABI major/minor, canonical ABI schema hash, required exports,
required imports (empty in v1), enabled Wasm features, and SDK version. Major mismatch rejects.
Core may run a connector with an older minor only when every emitted/received variant is supported.

### `mav:fixtures`

Contains bounded cases with an initial state, ordered input events, expected ordered actions,
expected final state hash, maximum fuel, and optional expected normalized samples/diagnostics. Raw
fixtures are versioned and content-hashed. Install runs all required cases in a fresh namespace; no
test can use network, filesystem, native BLE, or installed user state.

### `mav:signature`

Contains schema id, `Ed25519`, publisher key id, SHA-256 digest, and 64-byte signature. Signing is
separate from mobile app signing.

Canonical unsigned bytes are the original module bytes with the entire unique `mav:signature`
section removed, preserving every other byte and section order. The signed digest is:

```text
SHA-256("mavconn-signature-v1\0" || canonical_unsigned_module_bytes)
```

Ed25519 signs those 32 digest bytes. Validator recomputes and constant-time compares the digest,
then verifies the signature. Packer always appends signature last, rejects an existing signature,
and verifies its own output. This avoids reserializing Wasm and avoids the signature signing itself.

Publisher keys are distinct from registry and app-release keys. A key record has stable id,
public key, scope, validity interval, status, replacement id, and revocation reason/time. Rotation
requires old-key cross-signature or registry-root authorization. Revocation policy is cached and
versioned; lack of network cannot silently turn a revoked key trusted. Previously installed
connectors remain disabled or quarantined according to signed policy, with explicit diagnostics.

## ABI v1

V1 has no host imports. A connector exports memory plus:

```text
mav_abi_version() -> i64              # packed major/minor
mav_alloc(len: i32) -> i32
mav_dealloc(ptr: i32, len: i32)
mav_init(ptr: i32, len: i32) -> i64   # packed output ptr/len
mav_handle(ptr: i32, len: i32) -> i64
mav_snapshot() -> i64
```

Inputs/outputs are deterministic CBOR. Packed pointer/length uses unsigned high/low 32-bit halves,
specified by SDK tests. Host validates ranges, overlap, allocation accounting, output length, CBOR,
and action count before copying. It calls `mav_dealloc` after copying. A trap, invalid pointer,
oversized output, malformed message, fuel exhaustion, or unexpected export fails that invocation
without unwinding across FFI.

### Events

Closed versioned event families:

- lifecycle: `Init`, `Activate`, `Deactivate`, `Suspend`, `Resume`, `Cancel`, `RestoreState`;
- discovery: `Advertisement`, `ScanStopped`, `ServicesDiscovered`, `IdentityRead`;
- transport: `Connected`, `PairingResult`, `MtuChanged`, `Subscribed`, `Unsubscribed`, `ReadResult`,
  `WriteResult`, `Notification`, `Disconnected`, `TransportError`;
- time: `TimerFired` with opaque token and monotonic ordering only;
- persistence/pipeline: `StateCommitted`, `SamplesCommitted`, `SamplesRejected`;
- update: `PrepareStateMigration`, `StateMigrationCommitted`.

Each carries connector/session ids, an event sequence, cancellation generation, bounded byte fields,
and only the data needed for that event. UUIDs are normalized strings at the ABI edge. Host wall
time may accompany evidence/sample events as an explicit value; connectors cannot query a clock.

### Actions

Closed actions:

- `StartScan`, `StopScan`, `Connect`, `EnsurePaired`, `DiscoverServices`, `Subscribe`, `Unsubscribe`,
  `Read`, `Write`, `Disconnect`;
- `SetTimer`, `CancelTimer`;
- `StatePut`, `StateDelete`, `StateCommit`;
- `EmitSamples`, `EmitDiagnostic`, `DeclareCapabilities`, `CompleteOperation`.

Actions are ordered and bounded. Core executes one action at a time and returns its result before
continuing where a barrier is required. `EmitSamples` must reach `SamplesCommitted` before a later
device acknowledgement write can execute. The host rejects undeclared characteristic ids,
unsupported properties, forbidden operations, stale session/cancellation ids, unbounded values,
and impossible lifecycle transitions. Native layers cannot retry on their own.

## Errors, deadlines, and cancellation

Artifact, trust, ABI, trap, limit, lifecycle, transport, state, sample-admission, update, and
revocation failures have stable core error codes. Connector diagnostics are untrusted data: host
rate-limits, sizes, redacts, and namespaces them, then maps only safe summaries to UI. A connector
trap fails its current operation/session; it never panics core or poisons another instance.

Host assigns operation ids and deadline tokens. Connector requests only a named host limit profile
and opaque timers; it cannot read elapsed or wall time. Timer expiry returns `TimerFired`. User,
platform, disconnect, suspend, update, and removal cancellation increments a session generation,
returns `Cancel`, invalidates queued work, and rejects late native results. Cancellation is
idempotent. Core may force-terminate an instance after its bounded cancellation event; teardown does
not depend on connector cooperation. Protocol-specific retry decisions live in connector state,
while core caps actions, outstanding ops, timer count, fuel, and total session resource use.

## Runtime limits

WC-P0 froze development performance and footprint budgets in ADR-017. Later packets establish profiles
for artifact size, section size/count, module memories/tables/functions, linear memory, stack depth,
fuel per event and fixture, output bytes, action count, state bytes, diagnostic rate, and wall-time
watchdog. Limits are signed manifest profile names chosen from host-defined profiles; a connector
cannot request arbitrary larger values. Threads, shared memory, reference types not required by the
SDK, WASI, sockets, and start functions are rejected in v1.

The selected interpreter is pinned `wasmi` 1.1.0 with fuel enabled, parser/engine limits, store
memory limits, and extra runtime checks. WC-P0 passed iOS/Android Rust static builds and
representative realtime/history parity; the dependency first enters production in its owning runtime
packet.

## Installation API

Future shared-core API, exposed through UniFFI as byte/string records only:

```text
inspect_connector(bytes, source) -> InspectionReport
install_connector(bytes, source, approval_token) -> InstalledConnector
list_connectors() -> [InstalledConnector]
activate_connector(connector_id, version) -> ActivationResult
rollback_connector(connector_id) -> ActivationResult
remove_connector(connector_id, remove_state) -> RemovalResult
set_publisher_trust(key_id, decision) -> TrustResult
refresh_revocations(bytes, source) -> RevocationResult
```

`ConnectorSource` records `kind` (`url`, `local_file`, `share`, `registry`, `bundled_test`), sanitized
locator/display label, acquired-at host time, optional expected digest, registry id, and provenance
chain. Core never fetches the URL or opens the path. It receives bytes already acquired by native
code. Reports include artifact hash, connector/publisher identity, capabilities, compatibility,
signature/trust result, fixture results, requested limit profile, warnings, and an expiring approval
token bound to exact bytes and policy revision.

Install stages bytes and metadata, verifies again, runs self-tests, snapshots old active artifact
and state, commits artifact/source/trust/test records atomically, then optionally activates. A failed
activation restores old artifact/state. Updates cannot silently downgrade. Explicit developer-mode
downgrade on Android is policy-gated and retains audit provenance. Uninstall cancels sessions,
removes artifact and source metadata, then either deletes or quarantines scoped state by explicit
choice.

## Import flows

URL flow: user supplies URL → native HTTPS client applies platform transport/security/size policy →
native receives bounded bytes and redirect/final-URL metadata → optional expected digest is checked
early → native calls `inspect_connector` → UI shows core-produced identity, publisher, capabilities,
warnings, and self-tests → explicit approval yields token → `install_connector` revalidates exact
bytes and commits atomically. Core never performs HTTP and GitHub raw is only another HTTPS source.

Local/share flow: document picker, open-in-place, AirDrop, Android content URI, or share intent gives
native a file handle/stream → native copies bounded bytes into app-private memory/storage while the
permission is valid → records sanitized source kind/display label, not a diagnostic-leaked absolute
path → calls the identical inspect/approval/install sequence. Source mechanism cannot bypass trust,
compatibility, fixture, or downgrade rules. Failure/cancel deletes staging bytes and leaves current
activation untouched.

## Connector persistence

State keys are scoped by `(connector_id, publisher_key_id, device_id, state_schema)`. Connector code
never chooses another namespace. Values and aggregate namespace bytes are bounded. Writes stage
within one event and become visible only at `StateCommit`. Core journals schema, digest, source
version, and migration result.

Update runs `PrepareStateMigration` in an ephemeral copy. Success returns bounded replacement state
and a deterministic hash; core commits it with artifact activation. Failure leaves prior artifact
and state active. Rollback restores the prior snapshot. Cross-publisher state adoption is forbidden
without explicit user-approved transfer metadata.

## Discovery and connection lifecycle

Core selects installed connectors whose signed declarative advertisement rules match. Ambiguous
matches are surfaced; core does not guess by device family. Selected connector receives the
advertisement and drives connection through actions. Native executes generic actions only. Service
discovery results return as data, so connector can verify firmware/generation-specific layouts.

Session state is explicit: installed, selected, scanning, connecting, discovering, pairing,
configuring, streaming, historical, suspending, disconnected, failed. Connectors may refine their
private protocol state but cannot bypass host state. Every action has operation id, deadline token,
and cancellation generation. Disconnect or app suspension invalidates outstanding generations;
late native results are logged and ignored. Restoration creates a new instance, sends scoped state
and normalized platform restoration facts, and requires connector to re-establish subscriptions.

## SDK and tools

`mav-connector-sdk` provides no-std-friendly ABI types, deterministic CBOR, export macros, allocator
glue, bounded action builders, state-machine helpers, diagnostics, test harness, fixture authoring,
and artifact metadata macros. It contains no WHOOP UUID, command, retry, record, or generation rule.

Tooling provides:

- `mavconn-pack`: build, canonical-section injection, fixture embedding, signature creation;
- `mavconn-inspect`: metadata/signature/hash/limits display without execution;
- `mavconn-validate`: identical host validation and self-tests;
- `mavconn-test`: native unit plus Wasm parity/state scripts;
- registry publish command: digest-addressed upload and signed index update.

Automated architecture gates inspect Cargo edges, SDK dependency trees, Wasm imports/exports and
features, custom-section determinism, repository stale names, generated FFI, and packaged artifact
self-tests. They fail if core/FFI/frontends link a device crate, if a connector path-depends on
Maverick internals, or if a generic module contains a device UUID/opcode allowlist.

## Registry

Registry is discovery metadata, not an execution privilege. Signed index entries bind connector id,
version, artifact digest/URL/size, publisher key, ABI/core ranges, release channel, supersedence,
and revocation status. Clients download bytes through native networking, then use normal inspect and
install APIs. Direct URL and local imports remain first-class. Registry compromise cannot forge a
publisher signature; publisher compromise is handled by revocation and rotation.

## Platform policy

iOS defaults to official reviewed publishers and may disable arbitrary URL/local activation if App
Review evidence requires it. Android may allow user-approved third-party keys and sideloading.
Both parse the same artifact, use the same core verifier/runtime, show connector identity and
capabilities, and retain source provenance. No compiled-in proprietary fallback is the long-term
escape hatch.

## Security and validation

Required suites cover malformed Wasm/CBOR/sections, duplicate/signature confusion, invalid pointers,
traps, infinite loops, memory growth, output/action bombs, undeclared BLE access, stale results,
cross-connector state attempts, fixture lies, corrupted state, cancellation, reconnect storms,
upgrade/downgrade/rollback, revocation, publisher rotation, and deterministic cross-platform replay.
Fuzz parsers and ABI message boundaries. Run malicious connectors under the same production limits.

Performance gates measure cold parse/verify/instantiate, warm event latency, fuel per representative
notification, sustained realtime throughput, history burst throughput, peak/steady memory, artifact
size, binary-size delta, and battery/thermal behaviour on both platform classes. Thresholds are set
from WC-P0 evidence and may only tighten with final-device measurements.

## Unresolved evidence gates

- `wasmi` passed WC-P0 mobile-target, limit, parity, and overhead probes; interpreter replacement
  requires equivalent vectors and an ADR amendment.
- P0 development budgets are frozen in ADR-017. Final device energy, thermal, linked-size, and
  crash-recovery gates remain owned by WC-P13, WC-P14, and WC-P16.
- Apple Guideline 2.5.2 acceptance for remotely acquired interpreted connectors requires review or
  counsel evidence before iOS release; official-only/disabled remote activation is fallback policy.
- CBOR/crypto/Wasm parser crates require dependency, maintenance, fuzz, and platform audits in owning
  packets before versions freeze.
- Initial official publisher roots, offline revocation freshness, registry operator/hosting, and
  recovery after publisher-key loss need operational evidence before WC-P15 ships.
- WHOOP manifest/reference conflicts, MG/deep-stream gating, gen4 temperature calibration, and
  hardware restoration remain confidence-tagged until traceable captures adjudicate them.
- Component Model/WIT is not ABI v1 because current interpreter/tooling evidence is insufficient;
  adopting it later is an ABI-major ADR, not an invisible encoding change.

## Migration

WHOOP 4 and WHOOP 5 become separate public-SDK projects in `sennnen/maverick-connectors`. Pure
protocol logic may be shared as source libraries there. Native compiled code remains only behind a
fixture parity adapter, with owner and deletion packet. The switch requires frozen native-vs-Wasm
outputs, connection-state traces, history persist-before-ack tests, and both platform paths. WC-P12
deletes the compiled crate and all registration hooks. WC-P16 performs whole-application cleanup and
proves no permanent dual architecture remains.
