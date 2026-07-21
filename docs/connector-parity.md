# Connector parity evidence

WC-P11 freezes target-consistency evidence for the two first public connector artifacts. The source
of truth is the generated `mavconn-parity/v1` report beside each signed package in
`maverick-connectors@1158ce6`; exact artifact bytes and reports are mirrored under
`fixtures/connectors/` so Maverick can prove the runtime without linking connector source.

## What is frozen

For every embedded case, native connector tests execute the same ordered events and assert the
exact action batches and final state. The no-JIT runtime then executes the signed Wasm artifact and
records SHA-256 of canonical input events, ordered action batches, emitted samples, and final state,
plus the maximum fuel consumed by one call and peak linear-memory bytes. Both artifacts include
history cursor/retry, state restart, and malformed-frame cases in addition to admitted record and
stream fixtures.

| artifact | fixtures | SHA-256 | max fuel/call | peak linear memory |
|---|---:|---|---:|---:|
| WHOOP 4.0 | 14 | `ea7e360add1365a2ca8e1f06bb5631cda25fda93c601bd90b6b6f000a22e4df0` | 89,074 | 1,179,648 B |
| WHOOP 5.0/MG | 12 | `7829241ae70b256eb84ab70a9b8a5eac44512009fcf15aba5967cb35df94221d` | 3,631,187 | 1,245,184 B |

The runtime test regenerates each report from the committed artifact and demands byte equality. The
Swift and Kotlin tests independently read the same report schema, ids, hashes, required flow names,
and resource ceilings. Platform CI executes those tests while building the real iOS and Android
packages.

## Host timing profile

A disposable release-mode harness measured 100 parse/compile/instantiate/activate runs and 1,000
warm activation calls on an arm64 Apple M1 MacBook Air running Darwin 25.5.0:

| artifact | bytes | cold mean | cold p95 | warm p95 |
|---|---:|---:|---:|---:|
| WHOOP 4.0 | 199,893 | 2,756 µs | 4,404 µs | 27 µs |
| WHOOP 5.0/MG | 245,042 | 2,548 µs | 2,768 µs | 29 µs |

P0's 250 µs cold development gate was explicitly parameterized for its 8 KiB probe; applying that
number to 200–245 KiB production artifacts would hide the size difference rather than adjudicate
it. The full artifacts exceed that probe-only cold number but remain a one-time 2.5–2.8 ms mean
session cost, while warm p95, five-million fuel, four-MiB linear memory, and four-MiB artifact limits
all pass. Final device energy, thermal, linked-size, and timing measurements remain release gates.

## Local platform limits

The generated Swift parity test parses successfully with the installed compiler. Full iOS Rust/app
tests could not run locally because `xcode-select` points at Command Line Tools and the
`iphonesimulator` SDK is absent. Android Rust/app tests could not run because no SDK/NDK is installed
and the local JDK is 25.0.2 rather than the pinned 17. CI owns both full builds; hardware BLE checks
remain impossible until straps arrive. These are unexecuted gates, not passing evidence.

Parity is `[PROV]` consistency evidence. It proves native and Wasm targets agree; it does not turn
synthetic deep buffers into hardware evidence or validate physiological meaning.
