# Connector architecture audit

Audit date: 2026-07-18. Repositories were clean on `main` before planning began.

## Evidence inspected

- `sennnen/maverick` at `1dc4f10`: architecture documents, ADR-007/011/012/013/016, Cargo
  manifests, `mav-codec`, `mav-engine`, `mav-ffi`, `mav-replay`, WHOOP codec crate, tests, and both
  native shells.
- `sennnen/maverick-connectors` at `dfb351d`: both manifests, validator, README, and authoring guide.
- `tanarchytan/whoop-rs` at `375af9c8739304c105f9c8116b0038fe338e8488`: pure protocol,
  transport trait, client orchestration, UUID tables, fixtures, and architecture notes. It was
  cloned read-only to a temporary directory and not modified.
- `/Users/sennen/Developer/maverick-signing`: contains only `maverick-release.jks`. Its contents
  were not opened. It is an Android application-signing asset, not a connector trust store.

## Current architecture

The present system imports a JSON manifest, resolves its optional `codec` string against a table of
compiled Rust factories, builds a `RealtimeProcessor`, subscribes every manifest notify UUID, and
passes every notification into one decoder. `DeviceCodec` can decode a `RawFrame` with a manifest
and per-device KV store. It cannot receive service-discovery results, read/write completions,
pairing state, disconnects, timers, or cancellation, and it cannot return transport actions.

The design is boxed but not installable:

- `core/Cargo.toml` includes and exports `mav-connector-whoop`.
- `mav-ffi` and `mav-replay` depend on that crate.
- `MavRuntime::new`, `codec_for`, and replay construction name WHOOP explicitly.
- `HostRuntime::register_codec` stores built-in factories.
- Both external manifests say `"codec": "whoop"`; successful install depends on that compiled id.
- `docs/architecture.md`, `docs/connectors.md`, `docs/platform.md`, the root maps, and
  maverick-connectors authoring instructions all teach explicit compiled registration.

Adding a logical codec therefore requires core-repository edits, a rebuild, and app publication.
ADR-016 accurately calls this cost unavoidable for compiled code; it does not satisfy the revised
plugin product model.

## WHOOP leaks

Device protocol logic that must move into the two connector source projects:

- all files under `core/connectors/mav-connector-whoop/`;
- WHOOP factory selection and registration in `mav-ffi`, `mav-replay`, and `mav-engine` tests;
- WHOOP-specific convenience constructor `HandshakeConfig::gen5` in `mav-engine`;
- proprietary command assumptions that remain in generic acquisition/runtime paths;
- manifest schema names that exist only to dispatch compiled modules (`codec`, named WHOOP event
  vocabulary, and named compiled record decoders).

Product-shell references such as `"my-whoop"`, `Repository.whoopSource`, `EffortScale.whoop`, and
hard-coded WHOOP series queries are frontend leaks. They are recorded for future platform packets;
this planning task does not edit Swift or Kotlin.

Protocol examples and confidence-tagged source notes are not automatically leaks. A generic type or
test may cite WHOOP evidence without branching on WHOOP. Final cleanup distinguishes provenance from
runtime device knowledge.

## Reference comparison

| Area | Current Maverick/connectors | `whoop-rs` evidence | Migration implication |
|---|---|---|---|
| Family split | One compiled WHOOP codec serves both external manifests | Closed `Family` enum with per-generation framing, UUIDs, hello, and record dispatch | Ship two artifacts; share SDK/library code only where wire evidence agrees |
| Discovery | Service UUID and notify list in manifest | Scan both services; names mutable; serial from standard GATT is reliable | Advertisement and identity-read events/actions belong in ABI |
| Bond order | Runtime connects, then subscribes all notify UUIDs | Standard HR/battery subscribe, confirmed gen5 client hello, then encrypted vendor subscriptions; wrong order wedges link | Connector owns ordered state machine; host only validates/executes actions |
| Decode | Realtime, records, events, control, alarm/haptic builders are substantial | Adds hardware-verified framing, v18/v26, deep buffers, responses, config, hello, safety gates | Port selectively with fixtures; do not copy desktop transport code |
| History | Core has generic historical machinery plus WHOOP control decoder | Sans-IO `Offload`; start, record, HISTORY_END cursor echo, ACK, complete; 8 s inactivity abort | ABI needs timers, persist-before-ack ordering, and raw-frame diagnostics |
| Transport | Fixed start-scan/connect/subscribe/write action set | Reads, pairing, confirmed writes, MTU/discovery, per-characteristic notification routing | Expand normalized events/actions without exposing native BLE objects |
| Retry/reconnect | Generic command retry exists; host runtime is simpler | Protocol inactivity timer plus 3/6/12/24/48/60 reconnect policy | Generic timer primitive; retry policy remains connector state unless a true cross-device rule emerges |
| Persistence | Codec gets direct KV trait in-process | Per-strap protocol/client state plus calibration stores | Wasm gets scoped persistence actions, never DB or filesystem imports |
| Portability | Compiled crate uses Maverick types directly | `whoop-protocol` is sans-IO and portable; `whoop-client` depends on async transport/tokio/uuid; btleplug is desktop-specific | Port pure logic to SDK types; re-express client flow as event/action; exclude radio/tokio/CLI/store |
| Evidence quality | Many facts code-inferred and manifest-tagged | Current tree claims hardware-verified captures and 100 tests | Re-adjudicate fact by fact; update ledger tags only with traceable evidence |

## Missing or stale current behaviour

The current connector lacks a complete connection protocol, pairing order, standard-characteristic
reads, response collection, history offload driver, lossless unmapped-frame tap, raw-stream flow,
gated dangerous-command policy, and reference reconnect policy. Its gen5 enable sequence has ten
flags while the reference describes sixteen. Current manifests still carry `force_trim`, while the
reference treats force-trim as destructive and never sends it. These differences require explicit
fixture/hardware adjudication; neither repository is copied wholesale.

## Deletion inventory

Final migration deletes or redesigns:

- `core/connectors/mav-connector-whoop/` and its workspace/dependency entries;
- `DeviceCodec`, `ManifestCodec` as a device-plugin surface, `CodecFactory`, `register_codec`,
  `codec_for`, and compiled decoder admission lists;
- `codec` and compiled-module dispatch semantics in `connector-manifest/v1`;
- JSON folder import/validation as an installable connector format;
- explicit WHOOP registration and device conditionals in FFI/replay/engine/frontends;
- obsolete error codes, tests, docs, examples, feature flags, and dependency allowances;
- temporary native/Wasm parity adapters after WC-P12.

Useful frame, decode, protocol, and golden tests migrate to SDK or packaged-artifact tests before
their old files are removed.

## Required cleanup proof

Migration packets run repository-wide searches for `mav-connector-whoop`, `register_codec`,
`CodecFactory`, `codec_for`, `DeviceCodec`, `ManifestCodec`, `"codec": "whoop"`, `my-whoop`, and
device-family branches. Final dependency inspection must show no connector source linked into app
binaries and no edge crate naming a device. Remaining `whoop` matches must be either connector-repo
source, protocol evidence, user-facing identity text, or explicitly justified tests.
