# Native platform contract

This document is the boundary between the Rust core and the iOS and Android applications. The
boundary is intentionally small. The apps own radios, operating-system permissions, presentation,
localisation, and native ML execution. The core owns every order-sensitive data operation and every
health-related decision.

If a Swift or Kotlin change needs to know how a frame is decoded, which samples are valid, how days
merge, whether an analytic is available, or how a metric is computed, the boundary is wrong. That
knowledge belongs in Rust and crosses as a read model.

## Two surfaces, two jobs

`mav-ffi` exposes two separate surfaces.

The stateless fixture surface is:

```text
core_version() -> String
run_capture(manifest_json, capture_json) -> RunResult
```

It exists for replay, binding smoke tests, and cross-platform parity. It opens no product database
and owns no live session. The same manifest and capture must return the same session and analytics
hashes in Rust, Swift, and Kotlin.

The product surface is one stateful `MavRuntime` object. A runtime owns the database connection,
registered connector metadata, acquisition state, current session, pipeline state, bounded action
queue, and immutable host snapshot. Native code never holds a stage object or database handle.

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

Opening a newer database schema fails with `MAV-5003`. Opening or migrating any other invalid store
returns its existing stable storage code. A failed constructor never returns a half-open runtime.

## Connector registration

`install_connector(package_json)` validates and registers a package. Registration does not scan,
connect, or write to a device. The package must declare:

- connector id and version;
- manifest schema and core compatibility range;
- content hashes;
- identity, capabilities, GATT facts, framing, packet layouts, and command templates;
- signature metadata once package signing lands.

The runtime stores the validated package, not the untrusted input string. A connector cannot replace
another connector with a lower version unless a future explicit recovery operation permits it.
Removing a connector removes its ability to start new sessions; it never deletes user data.

The built-in standard Heart Rate Service connector follows the same registered representation even
though its package ships with the app. Proprietary connectors do not.

## Native transport events

The runtime accepts a closed set of typed UniFFI methods. It is not JSON and it is not an open RPC
surface. Named methods keep invalid payload combinations unrepresentable in Swift and Kotlin:

```text
start_scan(connector_id, device_id)
device_discovered(connector_id, native_device_id, display_name)
connected(native_device_id)
subscribed(characteristic)
notification(characteristic, bytes, at_unix_ms)
transport_failed(operation, native_code, safe_message, at_unix_ms)
disconnected(native_device_id)
```

Byte payloads cross as `Vec<u8>`, never hex strings. `native_device_id` is an opaque identifier used
only to route the current native connection; it is not treated as a stable physiological-device id.
Events that persist samples or diagnostics carry host time. Pure transport transitions do not read
the clock. The runtime validates state and rejects an impossible event with a stable transport error
instead of attempting to repair the sequence.

`notification` is the only entry for sensor and protocol bytes. It runs reassembly, decode, SQI,
timeline, storage, features, analytics, and snapshot publication in core-defined order. Native code
cannot call those stages individually.

Bond readiness, command completion, command responses, ticks, and reason-bearing disconnects are
added only when a real connector requires them. They must be narrow typed methods, not a generic
event map.

## Core transport actions

The host drains a bounded queue of closed `TransportAction` values:

```text
StartScan { service_filters }
StopScan
Connect { native_device_id }
Subscribe { characteristic }
Write { characteristic, bytes, with_response, sequence }
Disconnect { native_device_id, reason }
```

WHOOP command writes require `with_response = true`; a connector template cannot override a
core-enforced safety requirement. A drained action leaves the queue exactly once. Failed native
execution comes back as `TransportFailed`; native code does not silently retry. Retry limits,
sequence matching, and command order remain core policy.

Queue capacity is fixed at runtime construction. When the queue is full, the operation that would
enqueue another action fails with a new FFI or transport error. Accepted actions remain intact and
in order. Nothing is overwritten.

## Host snapshot

`host_snapshot(at_unix_ms)` returns `HostSnapshotResult`:

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

## Historical status

When M5-P7 lands, `historical` is `historical-status/v1` and contains:

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

The runtime is safe to hold from Swift and Kotlin, but it serialises mutation internally. Hosts call
it from one dedicated background executor. No runtime method is called from the UI thread.

Event application and action draining are synchronous and bounded. Database work is synchronous
inside that serialized call. Long recomputation and report export become explicit task operations
with progress snapshots rather than hidden work in a getter. Snapshot queries never mutate pipeline
state except for recording the supplied observation time.

Callbacks from Rust into Swift or Kotlin are not part of v1. Hosts poll by revision after applying an
event or on a platform-appropriate low-frequency timer. This avoids re-entrancy through UniFFI and
makes lifecycle behaviour testable.

## Native presentation boundary

Swift and Kotlin decode `host-snapshot/v1` into platform structs. A screen sees only those structs
and local display preferences.

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
- fill absent data with demo, cached NOOP, or guessed values;
- issue device commands not emitted by the runtime.

The four hub slots exist even when their analytics do not. Their state is one of value, collecting,
unavailable, or failed. That state comes from the core contract.

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
