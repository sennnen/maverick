# Maverick

Maverick (mav for short) is a wearable-data platform for iOS and Android that keeps your
data on your phone. A BLE strap streams to the device, a signed local connector decodes its raw
frames, the shared core computes everything from them, and none of it is sent anywhere. There is no account, no
server, and no cloud copy of your heart rate. The first straps it targets are the WHOOP 4.0
and the WHOOP 5.0/MG (those last two share a wire format; the 4.0 has its own).

Almost all of the work is a single Rust core shared by both platforms. Decoding, the data
pipeline, signal quality, storage, and the analytics all live there and run identically on
iOS and Android; the platform apps are thin shells that bind to the core over UniFFI and
render what it produces. One shared core means one place for a bug to live and one place to
fix it, rather than a Swift version and a Kotlin version that quietly drift apart.

## Status

Early. This is a from-scratch rewrite, and the human steering it does not have the hardware
yet, so nothing has been validated against a live strap. That constraint shapes the whole
project: the protocol side is built and tested against captured byte streams and facts read
out of two prior codebases, and every protocol claim carries a confidence tag that says
whether both sources agreed, only one did, or it is still a guess waiting on a real device.
When the straps arrive, verifying those tags becomes a checklist rather than a dig through
old memory. Work is core-first and proceeds in vertical slices with exit criteria, not dates.
There is no app to install today.

## How the repository fits together

The Rust workspace under `core/` is a set of small crates, each owning one job: frozen shared
types, connector verification/execution, the timeline, signal quality, features, analytics,
storage, observability, orchestration, the FFI facade, and replay without a radio. Device support
uses signed, runtime-loaded WebAssembly connectors: one `.mavconn` file runs through the same Rust
interpreter on iOS, Android, replay, and tests without rebuilding Maverick. The compiled WHOOP path
has been deleted; [the migration record](docs/plans/completed/wasm-connectors.md) preserves its
proof. Everything an agent or contributor needs is under `docs/`, the system of record. `CLAUDE.md`
(and identical `AGENTS.md`) is the short map.

## Building the core

You need a Rust toolchain; the version is pinned in `rust-toolchain.toml`. From `core/`:

    cargo test --workspace
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings

The same three commands, plus the documentation and dependency checks in `tools/`, are what
CI runs on every change. The BLE transport itself needs a phone and a strap and so is not
covered by these, but each connector state machine runs natively and under the production Wasm
interpreter against captured fixtures, so most protocol logic is reachable from `cargo test` alone.

## Where to start reading

Start with `docs/PLAN.md`. It lays out the principles, the milestone plan, and the work-packet
protocol that a single change is cut from. From there the map in `CLAUDE.md` will point you at
the specific document for whatever you are touching: the pipeline contracts, the connector
format, the WHOOP protocol notes, the testing policy, and so on.

---

Maverick is an independent rewrite in the spirit of earlier self-hosted WHOOP projects, built
fresh rather than forked. It is not affiliated with any of them, and not with WHOOP.
