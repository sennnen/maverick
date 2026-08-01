# Errors and observability

Error handling is milestone-zero work in Maverick, delivered before the first byte of protocol code,
and this document explains both the model and why it comes first. The short version of the why: this
codebase is written by a swarm of agents, and a swarm without forensics ships silent corruption. When
a decode goes subtly wrong on a normal team, someone eventually notices the odd chart and goes
digging. In a swarm, the agent that wrote the bug has no memory of it, the agent that touches the
code next has no suspicion, and a value that is wrong but plausible will flow through features into
metrics unchallenged. The only defence is that every anomaly leaves a record at the moment it
happens, with enough context to walk back to the bytes that caused it. That machinery cannot be
retrofitted; if the first milestones are built without it, the data they produced was never
witnessed, and there is no going back to check.

## MavError

Every fallible operation in the core returns `Result<T, MavError>`. The error type has five parts:

- **Code** — a stable numeric code, unique to one failure condition.
- **Category** — one of `Transport`, `Frame`, `Decode`, `Timeline`, `Storage`, `Feature`,
  `Analytic`, `Ml`, `Ffi`, `Connector`, `Internal`.
- **Message** — human-readable, specific, and stating what was expected against what was found.
- **Context chain** — the trail of what was being done when the failure happened, accumulated as
  the error propagates up: which device, which frame, which stage.
- **Severity** — how bad this is, from a logged-and-continue rejection up to a corruption risk.

The code is the part with the strictest rule: **codes are append-only.** A code, once assigned, is
never renumbered, never reused, and never deleted; a condition that stops being possible keeps its
code, marked retired. This is what makes a code stable enough to grep three months of journals for,
to cite in a fixture, and to reference from documentation. All codes are documented in this file as
they are assigned, next to the condition they name.

Each category owns a thousand-wide range, and the category is derived from the code in
`mav-model`, so the two can never disagree: Transport 1000–1999, Frame 2000–2999, Decode
3000–3999, Timeline 4000–4999, Storage 5000–5999, Feature 6000–6999, Analytic 7000–7999,
Ml 8000–8999, Ffi 9000–9999, Connector 11000–11999, and Internal for 10000 or otherwise
unassigned values.

## The code catalogue

The table below is the registry of every assigned code. It is kept mechanically in step with
`codes::ALL` in `mav-model` by a test in `mav-obs`; adding a code in one place and not the other
fails the build.

| code | name | condition |
|---|---|---|
| 1001 | TRANSPORT_COMMAND_TIMEOUT | a command's response never arrived after the retry budget was spent |
| 1002 | TRANSPORT_UNEXPECTED_RESPONSE | a response arrived that matched no outstanding command (wrong sequence, or none pending) |
| 1003 | TRANSPORT_UNEXPECTED_BYTES | data bytes arrived in a connection state that does not expect them |
| 1004 | TRANSPORT_HISTORICAL_PROTOCOL | historical control events arrived in an unsafe or impossible order |
| 1005 | TRANSPORT_COMMAND_REJECTED | a matched device command response carried a non-success result |
| 1006 | TRANSPORT_NATIVE_FAILURE | the native BLE stack reported that a requested transport operation failed |
| 2001 | FRAME_HEADER_CRC_MISMATCH | a frame header failed its CRC-8 (gen4) or CRC-16 (gen5) check |
| 2002 | FRAME_PAYLOAD_CRC_MISMATCH | a frame payload failed its trailing CRC-32 check |
| 2003 | FRAME_TRUNCATED | a declared frame length is shorter than its own CRC-32 field |
| 2004 | FRAME_OVERSIZED | a declared frame length exceeds the maximum frame size |
| 2005 | FRAME_GARBAGE_SKIPPED | bytes were discarded while scanning for a start-of-frame marker |
| 2006 | FRAME_READER_OUT_OF_BOUNDS | a field read ran past the end of a payload |
| 3001 | DECODE_UNKNOWN_PACKET_TYPE | a packet type has no entry in the device's manifest packet map |
| 3002 | DECODE_LAYOUT_INVALID | a manifest field layout is internally inconsistent or unusable |
| 3003 | DECODE_FIELD_UNREADABLE | a manifest-declared field could not be read from the payload |
| 3004 | RETIRED_DECODE_3004 | retired compiled-manifest registry code; numeric value remains reserved |
| 3005 | RETIRED_DECODE_3005 | retired compiled-record decoder code; numeric value remains reserved |
| 3006 | RETIRED_DECODE_3006 | retired compiled-codec admission code; numeric value remains reserved |
| 4001 | TIMELINE_IMPLAUSIBLE_TIMESTAMP | a device timestamp fell outside the plausible window and the sample was placed on capture time |
| 5001 | STORAGE_OPEN | the database could not be opened or initialised |
| 5002 | STORAGE_MIGRATION | a schema migration failed to apply |
| 5003 | STORAGE_NEWER_SCHEMA | the database schema version is newer than the code understands |
| 5004 | STORAGE_QUERY | a storage read or write failed |
| 5005 | STORAGE_SERIALIZE | a value could not be serialised for storage or read back |
| 8001 | ML_ECG_CAPTURE_STATE | an ECG capture command or transition is invalid for the current phase |
| 8002 | ML_ECG_INFERENCE_INVALID | native ECG inference returned the wrong count, shape, range, order, hash, or a non-finite value |
| 8003 | ML_ECG_PREPROCESSING | a captured ECG could not be converted into the admitted model tensor |
| 9001 | FFI_RUNTIME_STATE | a host-runtime operation was called in a state where it is not valid |
| 9002 | FFI_ACTION_QUEUE_FULL | a host-runtime transport action could not be queued without exceeding the fixed capacity |
| 9003 | FFI_CONNECTOR_NOT_FOUND | a host-runtime operation named a connector that is not registered |
| 9004 | FFI_CONNECTOR_DOWNGRADE | connector registration attempted to replace an installed version with an older one |
| 10000 | INTERNAL_INVARIANT | a state the code treats as impossible was reached |
| 11001 | CONNECTOR_ARTIFACT_OVERSIZED | a connector artifact exceeds the pre-parse byte limit |
| 11002 | CONNECTOR_ARTIFACT_MALFORMED_WASM | artifact bytes are not one structurally valid WebAssembly module |
| 11003 | CONNECTOR_ARTIFACT_SECTION_MISSING | a required mav metadata section is absent |
| 11004 | CONNECTOR_ARTIFACT_SECTION_DUPLICATE | a mav metadata section name appears more than once |
| 11005 | CONNECTOR_ARTIFACT_SECTION_ORDER | required mav metadata sections are not the final ordered section sequence |
| 11006 | CONNECTOR_ARTIFACT_UNKNOWN_CRITICAL_SECTION | an unknown mav:critical custom section requires unsupported behaviour |
| 11007 | CONNECTOR_ARTIFACT_SECTION_OVERSIZED | a custom section payload exceeds its pre-decode byte limit |
| 11008 | CONNECTOR_ARTIFACT_NONCANONICAL_CBOR | a required metadata payload is malformed, out of bounds, or not its canonical CBOR encoding |
| 11009 | CONNECTOR_ARTIFACT_DIGEST_MISMATCH | the signed digest differs from the canonical unsigned module digest |
| 11010 | CONNECTOR_TRUST_UNKNOWN_PUBLISHER | no trust-policy key matches the signature publisher id |
| 11011 | CONNECTOR_TRUST_KEY_NOT_YET_VALID | the publisher key validity interval has not started |
| 11012 | CONNECTOR_TRUST_KEY_EXPIRED | the publisher key validity interval has ended |
| 11013 | CONNECTOR_TRUST_KEY_REVOKED | key status or the active revocation set revokes the publisher key |
| 11014 | CONNECTOR_TRUST_KEY_ROTATED | the artifact uses a retired publisher key with a named replacement |
| 11015 | CONNECTOR_TRUST_SCOPE_REJECTED | platform policy does not allow this publisher scope |
| 11016 | CONNECTOR_TRUST_SIGNATURE_INVALID | Ed25519 verification failed for the signed digest |
| 11017 | CONNECTOR_TRUST_POLICY_INVALID | publisher ids or validity intervals make the trust policy ambiguous or unusable |
| 11018 | CONNECTOR_TRUST_REVOCATION_STALE | the revocation set is not yet valid, expired, or internally inverted |
| 11019 | CONNECTOR_RUNTIME_LIMIT_PROFILE | the signed resource profile is unknown or differs from the selected host profile |
| 11020 | CONNECTOR_RUNTIME_IMPORT_FORBIDDEN | the module imports a host, WASI, native, or other forbidden symbol |
| 11021 | CONNECTOR_RUNTIME_FEATURE_FORBIDDEN | the module uses a start function, shared or 64-bit memory/table, or another disabled Wasm feature |
| 11022 | CONNECTOR_RUNTIME_EXPORT_INVALID | a required ABI export is missing, has the wrong kind/signature, or reports the wrong ABI version |
| 11023 | CONNECTOR_RUNTIME_MODULE_LIMIT | static function, global, table, memory, element, or data counts exceed the selected profile |
| 11024 | CONNECTOR_RUNTIME_INSTANTIATION | bounded Wasm compilation or instantiation failed before a connector call |
| 11025 | CONNECTOR_RUNTIME_FUEL_EXHAUSTED | one connector call consumed its deterministic fuel allowance |
| 11026 | CONNECTOR_RUNTIME_STACK_LIMIT | connector recursion or value-stack use exceeded the selected profile |
| 11027 | CONNECTOR_RUNTIME_RESOURCE_LIMIT | runtime memory, table, or instance growth exceeded the selected profile |
| 11028 | CONNECTOR_RUNTIME_TRAP | connector code trapped for a reason other than a separately classified limit |
| 11029 | CONNECTOR_RUNTIME_MEMORY_ACCESS | an ABI pointer, length, overlap, read, or write was outside guest memory |
| 11030 | CONNECTOR_RUNTIME_INPUT_OVERSIZED | canonical event input exceeded the selected profile before allocation |
| 11031 | CONNECTOR_RUNTIME_OUTPUT_OVERSIZED | an action-batch output length exceeded the selected profile before copying |
| 11032 | CONNECTOR_RUNTIME_OUTPUT_INVALID | guest output was empty, malformed, noncanonical, or outside ABI action bounds |
| 11033 | CONNECTOR_RUNTIME_STATE_OVERSIZED | snapshot output exceeded the selected state bound before copying |
| 11034 | CONNECTOR_RUNTIME_INSTANCE_UNUSABLE | a prior hostile failure invalidated the instance for later calls |
| 11035 | CONNECTOR_RUNTIME_INPUT_INVALID | the host supplied an event that failed canonical ABI validation before guest allocation |
| 11036 | CONNECTOR_RUNTIME_FIXTURE_INVALID | an embedded fixture has no events, mismatched event/action counts, or an unusable fuel bound |
| 11037 | CONNECTOR_RUNTIME_FIXTURE_MISMATCH | Wasm actions or final snapshot hash differ from the signed embedded fixture |
| 11038 | CONNECTOR_HOST_STATE | a normalized event or connector action is invalid in the current lifecycle state |
| 11039 | CONNECTOR_HOST_ACTION_INVALID | an action batch has invalid context, shape, or chaining behaviour |
| 11040 | CONNECTOR_HOST_ACTION_UNDECLARED | an action exceeds signed transport, characteristic, or stream declarations |
| 11041 | CONNECTOR_HOST_QUEUE_FULL | a connector action batch cannot enter the fixed transport queue atomically |
| 11042 | CONNECTOR_HOST_RESULT_MISMATCH | a native result differs from its pending operation kind or characteristic |
| 11043 | CONNECTOR_HOST_SAMPLE_INVALID | an emitted sample cannot enter the frozen pipeline vocabulary or bounds |
| 11044 | CONNECTOR_HOST_LATE_RESULT | a cancelled, completed, or unknown result was journaled and ignored |
| 11045 | CONNECTOR_HOST_OPERATION_DUPLICATE | operation/deadline ids repeat, are zero, or exhaust the session budget |
| 11046 | CONNECTOR_INSTALL_APPROVAL_INVALID | install approval expired or no longer binds the artifact, source, or trust revisions |
| 11047 | CONNECTOR_INSTALL_DOWNGRADE | requested connector version is older than the active semantic version |
| 11048 | CONNECTOR_INSTALL_NOT_FOUND | requested installed, active, or rollback connector version does not exist |
| 11049 | CONNECTOR_INSTALL_STATE_NAMESPACE | connector state is invalid, oversized, or outside the exact active namespace |
| 11050 | CONNECTOR_INSTALL_MIGRATION | state migration failed or activation attempted to skip a required migration |
| 11051 | CONNECTOR_INSTALL_STORAGE | connector lifecycle schema, query, transaction, or stored value is invalid |
| 11052 | CONNECTOR_REGISTRY_OVERSIZED | a signed registry envelope or one of its bounded collections exceeds the accepted limit |
| 11053 | CONNECTOR_REGISTRY_MALFORMED | registry JSON is noncanonical, structurally invalid, or contains invalid metadata |
| 11054 | CONNECTOR_REGISTRY_SIGNATURE_INVALID | the configured registry root cannot verify the deterministic index digest |
| 11055 | CONNECTOR_REGISTRY_STALE | a registry or revocation index is outside its signed freshness interval |
| 11056 | CONNECTOR_REGISTRY_ROLLBACK | a registry revision or revocation revision was replayed or moved backward |
| 11057 | CONNECTOR_REGISTRY_CHAIN_INVALID | a refreshed index does not name the exact previous signed-index digest |
| 11058 | CONNECTOR_REGISTRY_ROTATION_INVALID | a publisher rotation is ambiguous or lacks a valid old-key cross-signature |
| 11059 | CONNECTOR_REGISTRY_ARTIFACT_MISMATCH | downloaded connector bytes differ from the registry entry digest or size |
| 11060 | CONNECTOR_REGISTRY_UPDATE_REJECTED | connector version, channel, supersedence, or downgrade policy rejects a registry update |
| 11061 | CONNECTOR_HOST_SAMPLE_DUPLICATE | emitted samples the pipeline already held; expected on a historical replay, recorded so nothing vanishes uncounted |
| 11062 | CONNECTOR_RUNTIME_SNAPSHOT_FAILED | the guest reported that building its snapshot failed; distinct from a legally empty snapshot |
| 11063 | CONNECTOR_HOST_DIAGNOSTIC_INFO | a connector diagnostic at info level, message carried through verbatim |
| 11064 | CONNECTOR_HOST_DIAGNOSTIC_WARNING | a connector diagnostic at warning level |
| 11065 | CONNECTOR_HOST_DIAGNOSTIC_ERROR | a connector diagnostic at error level |

Library code does not panic. `unwrap`, `expect`, and `panic!` are denied by the clippy
configuration for library code and allowed in tests. An impossible state is an `Internal` error
with a code, not a crash, because a crash in an FFI'd library takes the host app down with it.

`mav-connector-abi` is an isolated wire-schema leaf and deliberately does not depend on frozen
`mav-model`. It returns a closed `WireError` while decoding untrusted CBOR. `mav-connector-runtime`
maps every artifact, schema, trust, instantiation, resource, memory, ABI input/output, and fixture
failure into append-only Connector `MavError` codes before rejection can enter the journal or cross
FFI.

## No silent drops

Stated in [pipeline.md](pipeline.md) and enforced here: nothing is discarded without a record. Every
dropped packet, fragment, frame, or sample logs its error code and reason at the point of the drop.
A frame that fails CRC, a fragment the reassembler skips, a sample rejected by a plausibility gate,
an RR interval outside the valid window: each one is an entry in the journal, not an absence. The
discipline pays off in both directions. When data is missing, the journal says why; when the journal
is quiet, missing data means the device genuinely did not send it, and that distinction is the
difference between debugging and guessing.

## Tracing

The core uses the `tracing` crate, with one span per stage per unit of work. Ids (frame, session,
stream, device) are attached as span fields, so a log line is never orphaned from the data it
concerns. Because the pipeline is a synchronous call graph, spans nest in exactly the order the
stages ran, and a trace of one frame's journey reads top to bottom without interleaving.

## Sinks

There are two, chosen for different questions.

The **ring buffer** is in-memory, cheap, and always on. It holds the recent past at full detail and
answers "what just happened", which is what you want mid-session and what the report bundle
snapshots when the user hits a problem.

The **error journal** is a table in SQLite, written through `mav-store`, and holds errors durably
across restarts. It answers "what has been happening", which is what you want when a user reports
that last Tuesday's sleep looks wrong. Journal entries carry their code, category, severity,
context, and the relevant ids, so a journal row can be joined back to the samples and frames it
describes.

Beyond the sinks, the `Tap` trait (described in [pipeline.md](pipeline.md)) gives boundary-level
observation of the healthy path: counts and ids at every stage boundary, payload summaries in debug
builds. The taps are how you see what the pipeline is doing; the sinks are how you see what it
refused to do.

## The walk-back requirement

For any metric value on screen, it must be possible to walk backwards through stored ids: metric to
features, features to samples, samples to frames, frames to raw bytes. This is a requirement on the
whole system, checked in the hardening milestone, and it is what the provenance table in
[storage.md](storage.md) exists to serve. An error model that logs codes but cannot connect them to
data would be half the job.

## The report bundle

User-facing error reporting exists from day one, over FFI, in two forms.

First, a recent-errors query, so the apps can show an in-app diagnostics screen without any custom
plumbing.

Second, `export_report_bundle()`, which produces a zip containing: the app and core versions, the
device model, the manifest versions in use, a slice of the error journal, and the contents of the
ring buffer. The bundle is **redacted by default**: it contains no raw health samples unless the
user explicitly opts in. The versions matter as much as the errors, because a bug report that does
not pin the algorithm and manifest versions cannot be reproduced, and a manifest version can change
the meaning of a decode.

The bundle is designed for the failure mode this project will actually have: a user with a strap we
have never physically tested, seeing something wrong, and no way for us to attach a debugger. The
bundle is the debugger. If it is good, a protocol fact can be corrected from a single report; if it
is an afterthought, every field report is an unreproducible anecdote.
