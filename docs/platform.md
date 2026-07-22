# Native platform contract

This document is the boundary between the Rust core and the iOS and Android applications. The
boundary is intentionally small. The apps own radios, operating-system permissions, presentation,
localisation, and native ML execution. The core owns every order-sensitive data operation and every
health-related decision.

If a Swift or Kotlin change needs to know how a frame is decoded, which samples are valid, how days
merge, whether an analytic is available, or how a metric is computed, the boundary is wrong. That
knowledge belongs in Rust and crosses as a read model.

## Runtime surface

`core_version()` is the stateless binding smoke test. Artifact fixture replay belongs to
`mav-replay`; the product surface is one stateful `MavRuntime` object. A runtime owns the database connection,
installed connector artifacts, trust and source metadata, acquisition state, connector instance,
pipeline state, and bounded action queue. Native code never holds a stage, database, interpreter,
or connector handle. Signed `.mavconn` bytes are the only device execution path.

## Runtime construction

The product constructor accepts one `RuntimeConfig`:

| field | type | rule |
|---|---|---|
| `database_path` | string | absolute app-private path; the core creates or migrates it |
| `timezone_id` | string | IANA identifier supplied by the host; no core system-timezone reads |
| `transport_capacity` | u32 | bounded pending-action count; zero is invalid |
| `app_version` | string | included in diagnostics, not trusted for feature decisions |
| `app_build` | string | commit/run identifier included in diagnostics |

Production construction reads time only when the host supplies it on an event or query. Tests use
the same methods with fixed timestamps. There is no hidden system clock in a deterministic path.

Timezone data follows the same rule. When the core needs local calendar-day boundaries — the
affected-day recompute trigger after a historical sync — the host supplies an explicit UTC-offset
table (`mav-engine`'s `Timezone`: the IANA id plus ascending `OffsetSpan` entries derived from the
platform's own tzdb). The core does pure arithmetic on that table and never bundles or reads a
tzdb, so day boundaries are reproducible from the inputs alone and an OS tzdb update cannot move
a frozen fixture hash.

Opening a newer database schema fails with `MAV-5003`. Opening or migrating any other invalid store
returns its existing stable storage code. A failed constructor never returns a half-open runtime.

## Connector installation

All sources converge on byte-oriented core calls: inspect exact `.mavconn` bytes plus sanitized
source metadata, obtain an approval report/token, then install those same bytes. Core checks artifact
structure, deterministic metadata, compatibility, signature, publisher/revocation policy, Wasm
imports/limits, and embedded self-tests before atomic activation. It owns update, rollback, removal,
and connector-scoped state. Native code never validates a connector as authoritative.

WC-P7 exposes the complete management path on serialized `MavRuntime`:

```text
inspect_connector_bytes(bytes, source, policy, revocations, now_ms, approval_ttl_ms)
install_connector_bytes(request, policy, revocations)
list_installed_connectors()
activate_installed_connector(connector_id, version, policy, revocations, now_ms)
rollback_installed_connector(connector_id, policy, revocations, now_ms)
remove_installed_connector(connector_id, version, mode, policy, revocations, now_ms)
enforce_connector_trust(policy, revocations, now_ms)
```

`ConnectorSourceMetadata` contains only kind, a safe display label, and a 32-byte locator digest;
apps never pass a path or URL. Trust inputs contain public keys, scope, validity/status, and signed
revocation data. `ConnectorInspection` returns safe manifest fields, declared stream capabilities
and permissions, both digests, fixture count, and a 40-byte opaque one-time approval token. Native
approval UI shows those fields before it can submit the token. `ConnectorInstallRequest` groups
bytes, source, token, activation choice, and injected time so the large byte buffer crosses once per
call.

Every result is a value record: native code never receives a SQLite, artifact, Wasm, policy, or Rust
handle. The complete transaction rules are in [connectors.md](connectors.md).

## Native transport events

The WC-P5 core host accepts closed typed transport results and produces a bounded generic action
queue. It is not an open RPC surface. WC-P7 freezes and exposes the corresponding UniFFI records for:

```text
advertisement(...)
connected(...)
pairing_result(...)
services_discovered(...)
subscribed(...) / read_result(...) / write_result(...)
notification(...)
timer_fired(...) / cancelled(...)
transport_failed(...) / disconnected(...)
```

`open_connector_session(ConnectorSessionConfig, policy, revocations)` reverifies the active stored
artifact and fixtures, creates one bounded P5 host, and returns its lifecycle report.
`apply_connector_event`, `drain_connector_actions`, `cancel_connector_session`, and
`connector_lifecycle` are the only live-session operations. Opening a later session atomically
replaces the prior in-memory session. Activation, rollback, removal, or trust disable sends one
bounded cancellation, journals a hostile cancellation failure, and force-drops the old instance;
guest cooperation is never required for teardown.

Byte payloads cross as `Vec<u8>`/`Data`/`ByteArray`, never hex strings. A transport address is an
opaque identifier used only to route the current native connection; core does not treat it as a
stable physiological-device id.
Events that persist samples or diagnostics carry host time. Pure transport transitions do not read
the clock. The runtime validates state and rejects an impossible event with a stable transport error
instead of attempting to repair the sequence.

Core normalizes each result into the connector ABI, invokes the instance, validates returned
actions, and admits emitted samples through SQI, timeline, provenance, and transactional storage.
Native code cannot call stages or connector exports individually. Device protocol state remains
inside connector; lifecycle and resource policy remain in core. iOS and Android drive this host
through generic CoreBluetooth and BluetoothGatt executors.

Characteristic actions carry both the connector's logical id and the signed native service/
characteristic UUID pair resolved by the host. Native executors use UUIDs for CoreBluetooth or
BluetoothGatt and echo only the logical id in results. They therefore contain no device-family
tables, protocol UUID constants, opcodes, or parsers.

## iOS connector acquisition policy

WC-P13 routes document-picker files, share/open-in documents, and bounded HTTPS downloads through
one exact-byte acquisition type. Files larger than 4 MB are rejected before inspection; remote
responses are streamed into the same bound. Security-scoped URLs are held only for the read, and
the core receives a basename plus SHA-256 locator digest, never a path or URL.

The shipping Info.plist makes policy reviewable rather than implicit:

- `MAVConnectorManagerEnabled` gates the complete UI without changing connector storage.
- `MAVAllowRemoteConnectorImport` independently gates HTTPS acquisition if App Review requires it.
- `MAVOfficialPublisherKeys` is the release-owned public-key list; iOS sets `allow_third_party` and
  `allow_development` false regardless of import source. Import is never a signature bypass.

The document type is `dev.maverick.connector` / `.mavconn`. Background mode is only
`bluetooth-central`; `MavBluetoothExecutor` uses CoreBluetooth restoration with an opaque connector/
session/generation checkpoint and stores no artifact bytes, paths, URLs, protocol state, or private
key material. App Review acceptance evidence is the removal condition for retaining remote import;
until then the independent flag is the rollback point.

## Android connector acquisition policy

WC-P14 routes Storage Access Framework documents, VIEW/SEND content intents, shared HTTPS text, and
in-app HTTPS entry through the same exact-byte acquisition type. `BoundedConnectorReader` stops at
4 MB even when a malicious provider omits or lies about length; `HttpURLConnection` enforces HTTPS
after redirects and streams through that same bound. Core receives a display basename and SHA-256
locator digest, never a content URI, file path, or URL.

Android release configuration sets `MAV_CONNECTOR_MANAGER_ENABLED` and
`MAV_ALLOW_REMOTE_CONNECTORS` independently. `MAV_ALLOW_THIRD_PARTY_CONNECTORS` is false in release
and true only in explicitly labelled developer builds; either mode still requires a configured
publisher key plus normal core signature, fixture, compatibility, and revocation checks. Sideloading
an artifact cannot extend trust.

`MavBleExecutor` maps only host-resolved UUID actions to BluetoothGatt. It has no connector/device
tables, parsers, opcodes, retry policy, or physiological logic. The app requests only version-correct
BLE permissions, declares no storage permission, uses cleartext-disabled networking, and persists
only an opaque connector/session/generation checkpoint in private preferences. On process restart,
the manager reopens the verified session and lets connector actions re-establish transport; artifact
bytes and external locators are not persisted by the host shell.

## Signed registry acquisition

WC-P15 adds optional release-configured registry URL/id/root values to both shells. Each streams the
signed index under an independent 1 MiB bound, gives exact bytes plus its last verified checkpoint
to core, and persists only that public envelope/checkpoint. Offline restoration must match both byte
digest and monotonic revocation evidence. Registry artifacts are separately bounded, core-verifies
entry size/SHA-256, and then re-enter the normal publisher-signature, fixture, and explicit approval
path. Empty registry configuration disables discovery without disabling URL, file, document, or
share import.

## Core transport actions

The host drains a bounded queue of closed `ConnectorTransportAction` values:

```text
StartScan { service_uuids, manufacturer_ids }
StopScan
Connect { address }
EnsurePaired
DiscoverServices
Subscribe { characteristic_id, service_uuid, characteristic_uuid }
Unsubscribe { characteristic_id, service_uuid, characteristic_uuid }
Read { characteristic_id, service_uuid, characteristic_uuid }
Write { characteristic_id, service_uuid, characteristic_uuid, bytes, confirmed }
Disconnect
SetTimer { token, delay_ms }
CancelTimer { token }
```

Each value includes host-assigned operation id, deadline token, session id, and cancellation
generation. Signed characteristic declarations constrain each action. A connector cannot write an
undeclared characteristic or weaken required confirmed-write policy. A drained action leaves the
queue exactly once. Failed native execution returns as a typed event; native code never silently retries.
Protocol retry/sequence/order belongs to connector state, while core owns bounds, deadlines,
cancellation, and action validity.

Queue capacity is fixed at runtime construction. When the queue is full, the operation that would
enqueue another action fails with a new FFI or transport error. Accepted actions remain intact and
in order. Nothing is overwritten.

WC-P7 exposes every P5 request variant as `ConnectorTransportRequest`, wrapped by
`ConnectorTransportAction` with connector id, session id, cancellation generation, host operation
id, and deadline token. The matching `ConnectorTransportEvent` carries only generic advertisements,
transport results, notifications, timer results, and disconnect/error state. Raw handles, protocol
opcodes, device families, retry policy, URL clients, and file openers are absent from both bindings.

## Retired host snapshot evidence

ADR-017 removed the compiled-codec FFI snapshot getter. The schema below is frozen compatibility
evidence for test-only Swift/Kotlin decoders and has no production caller:

```text
json: String
hash: String
revision: u64
```

`json` is canonical `host-snapshot/v1`. `hash` is over those exact bytes. `revision` increments only
when the canonical snapshot changes. Repeated queries with the same runtime state and timestamp
return the same bytes, hash, and revision.

The first schema is an envelope around already admitted read models:

```json
{
  "schema": "host-snapshot/v1",
  "core_version": "0.1.0",
  "storage_schema": 1,
  "revision": 7,
  "as_of_unix_ms": 1784200000000,
  "connection": {
    "state": "streaming",
    "device_id": 1,
    "connector_id": "whoop5",
    "connector_version": "0.1.0",
    "display_name": "MG",
    "battery_percent": null,
    "charging": null,
    "on_wrist": null,
    "last_sample_unix_ms": 1784199999000
  },
  "session": {
    "schema": "snapshot/v1",
    "device_id": 1,
    "current_bpm": 63,
    "mean_milli_bpm": 61500,
    "in_range_samples": 4,
    "excluded_samples": 1,
    "provenance_id": 2
  },
  "analytics": {
    "schema": "analytics-snapshot/v1"
  },
  "historical": null,
  "recent_errors": []
}
```

`battery_percent` and `on_wrist` are the device's latest reported state of charge (whole percent,
`0..=100`) and wrist-worn flag, read from the stored WHOOP event stream (`BatterySoc` and
`WristState`, decoded from packet 48 by the `whoop` event vocabulary). Reading them from the store
rather than from memory means the value survives a runtime restart. Both are `null` until the device
sends the corresponding event. `charging` has no admitted decode yet and stays `null`.

The shortened analytics object above identifies nesting; the real object is the complete canonical
`analytics-snapshot/v1` value. Later milestones add day, sleep, strain, workout, and historical
read-model fields additively. They do not replace an absent value with a platform calculation.

Each analytic continues to carry structured availability. Native code renders that reason. It does
not infer availability from nulls. A null means the field has no value in that schema; availability
explains why.

Unknown top-level schema names are rejected by platform decoders. Unknown additive fields inside a
known schema are ignored so an older app can survive a newer compatible core. Missing required
fields fail decoding and surface a startup diagnostic. A breaking field change requires a new
schema name and parallel fixture.

## Retired historical-status evidence

The former snapshot nested `historical-status/v1` with:

- controller state;
- records seen, inserted, duplicated, and rejected;
- hash of the last durable cursor, never cursor bytes;
- sorted affected local days;
- stable failure code;
- last progress time.

Native code cannot emit an acknowledgement, construct a trim command, or pass a cursor back into
the runtime. It executes only the transport action the core emitted.

## Errors

FFI failures retain the stable `MAV-` code, category, severity, safe message, and context fields.
Flattening an error to one display string is allowed only at the final language exception boundary;
the product runtime returns structured errors for UI and reports.

`recent_errors` is bounded and redacted. It contains:

- code, category, and severity;
- safe message;
- stable next-action id;
- event time;
- connector, session, and stage ids where available.

It contains no raw health samples, command payloads, connector signatures, encryption material, or
historical cursor bytes. Localised prose belongs to the app; the stable code and next-action id keep
the two platforms behaviourally aligned.

## Threading and blocking

The runtime is safe to hold from Swift and Kotlin. Connector repository and active connector session
each serialize mutation behind a poison-safe mutex; concurrent management reads and writes return
typed results rather than racing SQLite or a Wasm instance. Hosts call it from one dedicated
background executor. No runtime method is called from the UI thread.

Event application and action draining are synchronous and bounded. Database work is synchronous
inside that serialized call. Long recomputation and report export become explicit task operations
with explicit progress rather than hidden work in a getter.

Callbacks from Rust into Swift or Kotlin are not part of v1. Hosts apply an event, drain the
resulting bounded actions, and repeat. This avoids re-entrancy through UniFFI and makes lifecycle
behaviour testable.

## Native presentation boundary

Swift and Kotlin screens see only value records returned by admitted core APIs plus local display
preferences. Frozen snapshot decoders are unreachable compatibility tests.

Native code may:

- select localized copy;
- format dates, durations, units, and fixed-point numbers;
- choose platform controls and accessibility behaviour;
- store appearance, unit, and card-visibility preferences.

Native code may not:

- compute or rescore a health metric;
- decide whether an analytic is available;
- query SQLite;
- merge days or remove duplicates;
- fill absent data with demo, cached legacy, or guessed values;
- issue device commands not emitted by the runtime.

The four hub slots exist even when their analytics do not. Their state is one of value, collecting,
unavailable, or failed. That state comes from the core contract.

### Unavailable analytics

`AuraUnavailableCard` — one component per platform, same wording — is how an absent metric is shown.
It takes the core's availability entry and renders the reason: what stream is missing, or that the
algorithm is not admitted. A screen never leaves an analytic blank and never substitutes a number it
computed for itself.

Zone math and the Tanaka HR-max ceiling come from the core over FFI (`heart_rate_zones`,
`heart_rate_zone_for`). The Kotlin ladder that used to duplicate them is gone.

### Design tokens

The Aura palette, spacing, radii, and type-scale sizes live once, in `tokens/aura.json`.
`tools/gen_design_tokens.py` (Python stdlib only, deterministic) generates
`apps/ios/Maverick/UI/AuraTokens.generated.swift` and
`apps/android/.../ui/aura/AuraTokens.generated.kt`; both files are committed, and the docs CI job
regenerates them and fails on a diff. Edit the JSON, run the generator, commit both.

The generator owns *values*, not decisions. Font families, weight names, and how a scheme is resolved
at runtime stay in the hand-written `AuraDesign.swift` and `AuraTheme.kt`, which consume the
constants — Helvetica Neue on iOS and the platform sans on Android is a deliberate difference, not
drift. So is Android's Material3 typography mapping, which has no iOS counterpart: those `.sp` values
are role names in a Compose API, not shared theme values.

The extraction audit found the two platforms already agreed on every colour, spacing, and radius, so
no divergence had to be resolved. That is worth stating rather than assuming: the value of this
pipeline from here on is that the next divergence fails CI instead of shipping.

Deliberately not built: any wireframe-to-code or layout-generation system. Two native shells that
already share an information model need a shared palette, not a shared renderer.

## Compatibility and fixtures

Every schema has:

- one canonical fixture;
- one frozen hash;
- a Swift decode test;
- a Kotlin decode test;
- a Rust round-trip test;
- a compatibility note when superseded.

Parity proves binding consistency, not physiological validity. Analytic validation still follows
[testing.md](testing.md): real capture or published reference first, platform parity second.
