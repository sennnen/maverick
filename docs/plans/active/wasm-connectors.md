# WC — Runtime-loaded WebAssembly connectors

Status: in progress. WC-P0 through WC-P13 are complete; WC-P14 is next.

This lane replaces ADR-016's compiled device codecs with signed, runtime-loaded `.mavconn`
artifacts under [ADR-017](../../adr/ADR-017.md). Architecture and current-state evidence are in
[connectors.md](../../connectors.md) and [connector-audit.md](../../connector-audit.md).

Every packet uses the work-packet protocol. Each leaves both repositories coherent and buildable.
One packet is one focused commit per touched repository. No packet pushes until its local checks
pass; GitHub Actions minutes are unavailable through 2026-08-01.

Common full gates in `maverick`:

```text
cd core && cargo test --workspace
cd core && cargo fmt --check
cd core && cargo clippy --workspace --all-targets -- -D warnings
tools/check_docs.sh
tools/check_deps.py
```

Common gates in `maverick-connectors` grow with the SDK, beginning with
`python3 tools/validate.py` and later adding workspace format, Clippy, tests, packaged validation,
and Wasm fixture parity. Packet-specific gates supplement, never replace, these.

## Packet WC-P0: Prove interpreter, limits, performance, and policy viability

Status: complete

- **Repositories touched:** `maverick` only.
- **Likely files/modules:** temporary `core/tools/wasm-probe/`, benchmark fixtures under
  `fixtures/probes/`, ADR-017 evidence appendix, and this decision log.
- **Dependencies introduced:** prototype-only pinned `wasmi`; none remain in production manifests
  unless accepted.
- **Public interfaces added:** none.
- **Old interfaces removed / obsolete files deleted:** none; probe source and generated artifacts
  are deleted before commit, retaining reproducible commands, hashes, and results.
- **Repository-wide searches:** confirm `wasmi` appears only in accepted dependency records after
  cleanup; confirm no JIT/runtime alternative leaked into product code.
- **Tests first:** infinite loop exhausts fuel; memory growth hits limit; malformed module is typed
  error; custom sections read before instantiation; representative realtime/history fixture output
  matches native reference.
- **Local validation:** build static Rust libraries for iOS device/simulator and Android ABIs;
  benchmark cold/warm latency, throughput, memory, binary-size delta, and extra-check overhead;
  run common gates. Record hardware/toolchain gaps exactly.
- **Acceptance:** deterministic failure modes, both mobile targets compile, representative event
  p95 stays within a budget derived and frozen in results, memory/binary size are acceptable, and
  App Review counsel/review experiment has an owner and release gate. Otherwise stop and amend ADR.
- **Rollback point:** documentation-only pre-runtime tree.
- **Risks:** interpreter target incompatibility, excessive binary/memory cost, Apple rejection.
- **Frontend:** explicitly deferred.
- **Temporary compatibility:** none.

## Packet WC-P1: Freeze artifact schemas and ABI wire types

Status: complete

- **Repositories touched:** `maverick`.
- **Likely files/modules:** new `core/crates/mav-connector-abi/`, schema fixtures, architecture edge
  table, `tools/check_deps.py`, errors ledger, ADR if frozen `mav-model` types must change.
- **Dependencies introduced:** deterministic CBOR/serde crates selected by P0; no interpreter.
- **Public interfaces added:** manifest/ABI/fixture/signature records; `ConnectorEvent`,
  `ConnectorAction`, ids, lifecycle states, packed pointer/length helpers, limits profile ids.
- **Old interfaces removed / obsolete files deleted:** none.
- **Repository-wide searches:** reject WHOOP/device names in new crate; audit all public enum
  exhaustiveness and serialization names.
- **Tests first:** byte-frozen CBOR vectors; duplicate/unknown fields reject; packed pointer round
  trip; every action/event has exact bounds; no float/non-deterministic map accepted.
- **Local validation:** crate tests, schema corpus, common gates.
- **Acceptance:** ABI v1 vectors and schema hashes freeze; crate has no device/runtime/native deps;
  dependency checker enforces edge.
- **Rollback point:** remove new leaf crate and schema fixtures.
- **Risks:** freezing too early; mitigate with only evidence-required WHOOP flows plus malicious cases.
- **Frontend:** deferred.
- **Temporary compatibility:** none.

## Packet WC-P2: Parse, inspect, hash, sign, and enforce trust

Status: complete

- **Repositories touched:** `maverick`.
- **Likely files/modules:** new `mav-connector-runtime` artifact/trust modules, inspection CLI/tests,
  error codes/docs, Cargo manifests and dependency edges.
- **Dependencies introduced:** Wasm parser, SHA-256, audited Ed25519 verifier, constant-time helpers.
- **Public interfaces added:** `Artifact::inspect`, canonical unsigned-module iterator,
  `InspectionReport`, `PublisherKey`, `TrustPolicy`, `RevocationSet`.
- **Old interfaces removed / obsolete files deleted:** none.
- **Repository-wide searches:** crypto algorithms/keys only in trust module; no release JKS refs;
  no parser accepts legacy JSON folders as `.mavconn`.
- **Tests first:** truncated/oversized/duplicate sections, noncanonical CBOR, digest mutation,
  signature self-exclusion, wrong key, expired/revoked/rotated key, unknown critical section.
- **Local validation:** corpus tests, parser/signature fuzz target smoke run, common gates.
- **Acceptance:** metadata and trust resolve without instantiation; every untrusted failure is typed;
  canonical test vectors are independently reproducible.
- **Rollback point:** remove runtime artifact/trust modules; ABI crate remains.
- **Risks:** parser differentials and signature ambiguity.
- **Frontend:** deferred.
- **Temporary compatibility:** none.

## Packet WC-P3: Ship public SDK, packer, inspector, validator, and test harness

Status: complete

- **Repositories touched:** `maverick` and `maverick-connectors`.
- **Likely files/modules:** SDK crate/API docs in `maverick`; connector repo Cargo workspace,
  `tools/mavconn-*`, template example, registry of schema vectors.
- **Dependencies introduced:** SDK CBOR/allocator support; CLI-only signing and Wasm section tooling.
- **Public interfaces added:** export macros, bounded builders, test driver, `mavconn-pack`,
  `mavconn-inspect`, `mavconn-validate`, `mavconn-test`.
- **Old interfaces removed / obsolete files deleted:** Python validator becomes legacy-only and is
  deleted in WC-P12 after both WHOOP projects package successfully.
- **Repository-wide searches:** SDK has no WHOOP/device constants; signing secret paths never enter
  config, output, fixtures, or git.
- **Tests first:** template compiles to `wasm32-unknown-unknown`; pack→inspect→verify round trip;
  deterministic repeated builds; malformed exports and oversized fixtures reject.
- **Local validation:** both common gate sets, install validator against packaged template.
- **Acceptance:** third-party example packages and validates without editing Maverick; artifact bytes
  are deterministic given same compiler/tool inputs and signing seed test vector.
- **Rollback point:** ABI/runtime inspection remains; remove SDK/tool workspace additions.
- **Risks:** proc-macro ABI drift, nondeterministic toolchain output.
- **Frontend:** deferred.
- **Temporary compatibility:** Python manifest validator; deleted WC-P12.

## Packet WC-P4: Instantiate hostile modules under deterministic limits

Status: complete

- **Repositories touched:** `maverick`; the `maverick-connectors` template was corrected when real
  execution exposed its exact feature/fuel declaration requirements.
- **Likely files/modules:** `mav-connector-runtime/{engine,instance,memory,limits}.rs`, malicious test
  modules, Cargo/dependency rules.
- **Dependencies introduced:** exact interpreter version accepted by P0.
- **Public interfaces added:** `ConnectorInstance::instantiate/init/handle/snapshot`, `LimitProfile`,
  trap/resource diagnostics.
- **Old interfaces removed / obsolete files deleted:** none.
- **Repository-wide searches:** WASI/network/fs/clock/random/thread imports absent; interpreter used
  only inside runtime crate.
- **Tests first:** import/start/shared-memory rejection; invalid pointers; output bombs; call-stack,
  memory, table, instance and fuel limits; trap isolation; deterministic repeated event traces.
- **Local validation:** malicious suite, fuzz smoke, P0 perf regression thresholds, common gates.
- **Acceptance:** hostile instance cannot hang, panic host, escape memory, or exceed profiles; failures
  do not poison another instance.
- **Rollback point:** artifact inspection remains functional without execution.
- **Risks:** interpreter bugs and resource accounting gaps.
- **Frontend:** deferred.
- **Temporary compatibility:** none.

## Packet WC-P5: Drive normalized event/action transport lifecycle

Status: complete

- **Repositories touched:** `maverick`.
- **Likely files/modules:** `mav-engine` connector host/session/action executor; runtime integration;
  generic transport model and tests; platform contract docs.
- **Dependencies introduced:** internal ABI/runtime edges only.
- **Public interfaces added:** normalized event entry, ordered action drain/results, operation ids,
  deadline tokens, cancellation generations, lifecycle snapshots.
- **Old interfaces removed / obsolete files deleted:** old simple runtime transport path is replaced
  where no compiled codec depends on it; fixture runner adapter remains until WC-P12.
- **Repository-wide searches:** generic engine contains no UUID/opcode/device family; native retries
  prohibited; all action variants capability-checked.
- **Tests first:** WHOOP-reference bond-order script expressed only as fixture data; wrong-order and
  undeclared actions reject; late results ignored; disconnect cancels; queue bounds; samples commit
  before later write.
- **Local validation:** engine tests, deterministic trace hashes, common gates.
- **Acceptance:** event/action script covers discovery→pair→subscribe→stream→disconnect and history
  barriers without host device logic.
- **Rollback point:** retain old stateful host runtime behind test-only adapter.
- **Risks:** accidentally encoding WHOOP as generic policy; cancellation races.
- **Frontend:** deferred.
- **Temporary compatibility:** old runtime adapter owned here; deleted WC-P12.

## Packet WC-P6: Install, activate, update, rollback, remove, and migrate state

Status: complete

- **Repositories touched:** `maverick`.
- **Likely files/modules:** new connector install/state tables and forward migration in `mav-store`
  or dedicated store crate; installer/service modules; storage/errors docs.
- **Dependencies introduced:** none beyond runtime/crypto already accepted.
- **Public interfaces added:** inspect/install/list/activate/rollback/remove/trust/revocation core
  APIs; source provenance; atomic state migration and audit records.
- **Old interfaces removed / obsolete files deleted:** manifest-only in-memory registration stops
  being product install path; compatibility wrapper remains for parity only until WC-P12.
- **Repository-wide searches:** no raw local path or URL in diagnostics; namespaces include connector,
  publisher, device, state schema; no cross-namespace lookup.
- **Tests first:** failed verify/self-test/activation leaves old version; downgrade refusal; crash at
  each transaction boundary; rollback restores state; uninstall cleanup/quarantine; key rotation and
  revocation disable policy.
- **Local validation:** migration round trips, failure injection, common gates.
- **Acceptance:** all lifecycle changes atomic and provenance-complete; restart restores active
  version and scoped state exactly.
- **Rollback point:** DB forward migration remains readable; feature disabled, prior active path used.
- **Risks:** state loss, partial activation, irreversible schema migration.
- **Frontend:** deferred.
- **Temporary compatibility:** manifest registration wrapper; deleted WC-P12.

## Packet WC-P7: Expose platform-neutral connector management over UniFFI

Status: complete

- **Repositories touched:** `maverick`.
- **Likely files/modules:** `mav-ffi`, binding vectors, docs/platform.md, generated-binding checks.
- **Dependencies introduced:** no new external dependency.
- **Public interfaces added:** byte/source inspection and install calls, approval tokens, connector
  list/activate/rollback/remove/trust APIs, generic transport events/actions.
- **Old interfaces removed / obsolete files deleted:** old manifest registration is marked
  migration-only; no Swift/Kotlin UI changes yet.
- **Repository-wide searches:** FFI names no device, codec id, URL client, or file opener; payloads
  cross as bytes/records, never raw Rust/Wasm handles.
- **Tests first:** Kotlin/Swift binding generation, byte round trips, stale approval token, concurrent
  calls, cancellation, safe error mapping.
- **Local validation:** Rust tests, binding generation/static compile where available, common gates.
- **Acceptance:** both shells can acquire bytes elsewhere and drive entire core API without protocol
  knowledge; current apps still build.
- **Rollback point:** leave new core APIs unused; old app runtime unchanged.
- **Risks:** FFI ownership and large byte-copy cost.
- **Frontend:** explicitly deferred to WC-P13/P14.
- **Temporary compatibility:** old FFI install method; deleted WC-P12 or owning platform packet if
  build ordering requires, with no production caller after WC-P14.

## Packet WC-P8: Build shared WHOOP reference library and adjudicated fixtures

Status: complete

- **Repositories touched:** `maverick-connectors`; protocol ledger updates in `maverick` only when
  evidence changes a tag.
- **Likely files/modules:** connector repo Rust workspace, private/shared `whoop-protocol` library,
  fixture provenance, comparison matrix.
- **Dependencies introduced:** SDK only plus no-std-compatible pure helpers.
- **Public interfaces added:** connector-internal pure framing, decode, command, response, offload
  helpers; not host ABI additions.
- **Old interfaces removed / obsolete files deleted:** no old manifests yet.
- **Repository-wide searches:** exclude btleplug/tokio/CLI/SQLite/native APIs and analytics from
  connector code; dangerous commands gated or absent.
- **Tests first:** current Maverick goldens plus independently sourced `whoop-rs` frames; generation
  framing, CRC, responses, history cursor, unmapped records, malformed inputs.
- **Local validation:** native unit tests, wasm compile/tests, both repo gates.
- **Acceptance:** every ported fact has source/fixture/confidence; shared library has no host/device
  access and no Maverick-core private dependency.
- **Rollback point:** manifests and compiled native connector remain authoritative.
- **Risks:** private-source copying, conflicting evidence, accidental analytics inclusion.
- **Frontend:** deferred.
- **Temporary compatibility:** shared library exists beside old compiled crate; removed only if no
  packaged connector uses it, otherwise remains connector-local.

## Packet WC-P9: Package WHOOP 4.0 as public connector

Status: complete

- **Repositories touched:** `maverick-connectors`.
- **Likely files/modules:** `connectors/whoop4/` Rust project, manifest metadata, state fixtures,
  package/sign config using test keys only in repo.
- **Dependencies introduced:** public SDK and shared WHOOP library.
- **Public interfaces added:** no host-specific additions; `.mavconn` artifact exercises public ABI.
- **Old interfaces removed / obsolete files deleted:** old `whoop4/manifest.json` retained only as
  input evidence and deleted WC-P12.
- **Repository-wide searches:** no gen5 fallback, no host/core path dependency, no secret key, no
  force-trim action.
- **Tests first:** advertisement/identity, unbonded standard chars, gen4 hello, subscriptions,
  realtime, v5/v7/v9/v12/v24/v25, events, history timeout/ACK ordering, disconnect/restore.
- **Local validation:** native/Wasm parity, packaged self-tests, validator, malicious inputs, connector gates.
- **Acceptance:** standalone signed test artifact installs and replays all admitted gen4 fixtures
  without Maverick source edits.
- **Rollback point:** artifact not activated; old path remains.
- **Risks:** hardware-unverified handshake and temperature calibration.
- **Frontend:** deferred.
- **Temporary compatibility:** old manifest/native path; deleted WC-P12 after parity.

## Packet WC-P10: Package WHOOP 5.0/MG as public connector

Status: complete

- **Repositories touched:** `maverick-connectors`.
- **Likely files/modules:** `connectors/whoop5/` Rust project, manifest metadata, state fixtures.
- **Dependencies introduced:** public SDK and shared WHOOP library.
- **Public interfaces added:** none beyond public ABI.
- **Old interfaces removed / obsolete files deleted:** old `whoop5/manifest.json` retained only as
  evidence and deleted WC-P12.
- **Repository-wide searches:** no gen4 fallback, no desktop transport/tokio, no hidden subscription
  gate claims, no release/signing secrets.
- **Tests first:** both service identities, proven bond order, confirmed client hello, encrypted
  subscriptions, realtime/event/response, v18/v26/deep buffers, R22 sequence, history idle/ACK,
  cancellation/reconnect/restore.
- **Local validation:** native/Wasm parity, packaged self-tests, validator, malicious inputs, connector gates.
- **Acceptance:** standalone artifact installs and replays all admitted gen5 fixtures; MG uncertainty
  remains tagged rather than guessed.
- **Rollback point:** artifact not activated; old path remains.
- **Risks:** firmware/subscription-specific behaviour, deep-buffer ambiguity.
- **Frontend:** deferred.
- **Temporary compatibility:** old manifest/native path; deleted WC-P12 after parity.

## Packet WC-P11: Prove native-versus-Wasm parity and cross-platform operation

Status: complete

- **Repositories touched:** both.
- **Likely files/modules:** replay parity harness, golden state traces, platform test harnesses,
  benchmark reports.
- **Dependencies introduced:** none.
- **Public interfaces added:** none.
- **Old interfaces removed / obsolete files deleted:** none until proof passes.
- **Repository-wide searches:** every temporary adapter carries `WC-P12` removal marker; no second
  production selector exposed.
- **Tests first:** byte/sample/snapshot hashes, ordered transport traces, history cursors, state
  restart, malformed frames, fuel/resource profiles on both artifacts and both platforms.
- **Local validation:** all repo gates; iOS simulator/device static test as available; Android JVM/
  emulator/device test as available; record unavailable hardware checks.
- **Acceptance:** frozen equivalence for admitted legacy behaviour; differences are adjudicated and
  documented, never normalized away; Wasm path meets P0 budgets.
- **Rollback point:** keep native path active and artifacts experimental.
- **Risks:** legacy bugs masquerading as required parity; platform timing differences.
- **Frontend:** test harness only; product UI deferred.
- **Temporary compatibility:** dual execution strictly in tests/dev; deleted WC-P12.

## Packet WC-P12: Switch active path and delete bundled connector architecture

Status: complete

- **Repositories touched:** both.
- **Likely files/modules:** Cargo workspace/manifests, `mav-codec`, engine, FFI, replay,
  `core/connectors/`, old connector folders/validator, tests, docs, errors, dependency checker.
- **Dependencies introduced:** none.
- **Public interfaces added:** packaged artifact becomes sole runtime path.
- **Old interfaces removed / obsolete files deleted:** entire `mav-connector-whoop` crate;
  `DeviceCodec` plugin trait/registry/factories; `register_codec`; `codec_for`; explicit edge deps;
  compiled decoder admission; legacy JSON folder import; Python legacy validator; parity adapters;
  stale errors/features/tests/docs.
- **Repository-wide searches:** exact deletion proof from connector-audit plus Cargo graph inspection;
  classify every remaining WHOOP occurrence.
- **Tests first:** app/replay starts with zero linked proprietary connectors; installing either
  artifact enables it; unknown third-party template installs; removed ids fail stale-reference gate.
- **Local validation:** both full repo gates, stale-reference script, `cargo tree`, binary symbol/name
  inspection where practical, packaged fixture suite.
- **Acceptance:** no device requires rebuild or registration; no permanent dual architecture; all
  useful legacy tests run through `.mavconn`.
- **Rollback point:** revert switch commit and reactivate prior binary; installed-artifact DB remains
  forward-readable. No partial deletion commit.
- **Risks:** hidden generated binding/build references, missed legacy behaviour.
- **Frontend:** management UI still deferred; test acquisition can install bytes.
- **Temporary compatibility:** none survives.

## Packet WC-P13: Implement iOS acquisition and connector management UI

Status: complete.

- **Repositories touched:** `maverick`.
- **Likely files/modules:** iOS document picker/share/open URL handling, connector store/view models,
  capability approval UI, generic BLE action executor, project configuration/tests.
- **Dependencies introduced:** Apple frameworks only; no Wasm/native connector runtime outside core.
- **Public interfaces added:** app-facing URL/file/share flows; no protocol APIs.
- **Old interfaces removed / obsolete files deleted:** WHOOP source constants/queries, device-specific
  transport/UI conditionals, stale generated FFI calls.
- **Repository-wide searches:** frontend has no WHOOP UUID/opcode/parser/codec; local paths sanitized;
  platform policy restricts unapproved publishers according to release evidence.
- **Tests first:** URL/local/share converge on identical bytes/source call; inspection before approval;
  cancel/failure/rollback/revocation UI; generic transport action mapping; background restoration.
- **Local validation:** iOS build/tests/static analysis, FFI parity, common gates.
- **Acceptance:** supported import sources and lifecycle work without device logic; App Review policy
  and official-publisher restriction are explicit release configuration.
- **Rollback point:** feature flag disables import/management while existing app remains usable.
- **Risks:** App Review rejection, background BLE restoration, security-scoped file access.
- **Frontend:** this is owning iOS packet.
- **Temporary compatibility:** release flag only; removal condition is accepted App Review evidence,
  otherwise retained as documented platform policy rather than alternate connector architecture.

Implemented 2026-07-21. File picker, open-in/share, and bounded HTTPS acquisition converge on one
exact-byte path with sanitized provenance. Inspection exposes publisher, capabilities, permissions,
and fixture proof before approval; install, list, rollback, quarantine removal, and revocation have
explicit UI states. Core-resolved native UUIDs close the generic transport boundary, and the
CoreBluetooth executor maps every action/event plus timers and opaque restoration checkpoints.
Release configuration accepts only configured official publishers and independently gates manager
and remote acquisition. Swift and generated bindings parse together; full Rust tests, fmt, clippy,
docs, dependency, plist, XcodeGen, and stale-reference checks pass. Simulator build/test remains
environment-blocked because only Command Line Tools is selected; `build_ios_app.sh` exits with the
explicit full-Xcode prerequisite before compilation.

## Packet WC-P14: Implement Android acquisition and connector management UI

Status: pending; future frontend work.

- **Repositories touched:** `maverick`.
- **Likely files/modules:** Android SAF/intents/share/URL acquisition, connector UI/view model,
  generic BLE executor, manifest/config/tests.
- **Dependencies introduced:** Android platform libraries only.
- **Public interfaces added:** app-facing import/management flows; optional trusted third-party mode.
- **Old interfaces removed / obsolete files deleted:** hard-coded `my-whoop` and device-specific
  transport/UI paths, stale bindings/resources.
- **Repository-wide searches:** no WHOOP UUID/opcode/parser/codec; intents bounded and provenance
  sanitized; sideload policy cannot bypass core signature checks.
- **Tests first:** URL/local/content/share equivalence; approval/cancel/failure/rollback/revocation;
  process death/restore; generic action mapping; malicious oversized content URI.
- **Local validation:** Gradle tests/lint/build, FFI parity, common gates.
- **Acceptance:** one artifact from any supported source installs via core; third-party trust remains
  explicit and audited.
- **Rollback point:** disable connector manager feature without DB rollback.
- **Risks:** content-provider lifetime, background restrictions, BLE vendor variance.
- **Frontend:** this is owning Android packet.
- **Temporary compatibility:** none.

## Packet WC-P15: Add signed registry, publishing, rotation, and revocation flow

Status: pending

- **Repositories touched:** both.
- **Likely files/modules:** registry schema/client in core, connector publish CLI/docs, signed test
  index, trust/revocation fixtures.
- **Dependencies introduced:** native networking stays frontend-owned; core gets only index parser/
  verifier already using accepted crypto.
- **Public interfaces added:** registry source metadata, signed index/refreshed revocation ingestion,
  publish command and key-rotation records.
- **Old interfaces removed / obsolete files deleted:** any ad hoc release list or URL convention.
- **Repository-wide searches:** core performs no network; registry key cannot sign connectors;
  secrets absent; direct URL/local path remains supported.
- **Tests first:** compromised index cannot forge publisher; rollback/freeze/replay protection;
  rotation and revocation offline cache; artifact digest mismatch; channel/update/downgrade policy.
- **Local validation:** deterministic index vectors, both repo gates.
- **Acceptance:** third party can publish without Maverick edit; clients still verify normal artifact
  signature and policy after download.
- **Rollback point:** disable registry discovery; direct import remains.
- **Risks:** stale revocation offline, key loss, registry rollback attacks.
- **Frontend:** registry browsing UI separate future product packet; byte acquisition only here.
- **Temporary compatibility:** none.

## Packet WC-P16: Whole-application cleanup, bug fix, and final proof

Status: pending

- **Repositories touched:** both; signing directory and `tanarchytan/whoop-rs` remain unchanged.
- **Likely files/modules:** any Rust/FFI/Swift/Kotlin/build/test/doc file with evidence-backed defect or
  stale architecture; focused commits by defect class.
- **Dependencies introduced:** none unless a discovered bug has separate justified packet/ADR.
- **Public interfaces added:** none by default.
- **Old interfaces removed / obsolete files deleted:** all confirmed dead features, exports,
  bindings, resources, fixtures, scripts, dependency edges, TODOs, telemetry/error names, and
  compatibility paths from migration.
- **Repository-wide searches:** full deletion inventory; WHOOP only in connector source/evidence or
  user-facing identity; no registration, linked connector crate, legacy manifest install, native
  decoder, device-specific frontend protocol, unowned temporary marker, unused dependency/feature.
- **Tests first:** reproduce each discovered bug before fix; architecture proof scripts fail on
  seeded stale reference.
- **Local validation:** format, Clippy warnings denied, all Rust tests, dependency/docs checks,
  SDK/Wasm/malicious/upgrade/cross-platform suites, iOS and Android builds/tests, FFI generation,
  stale-reference/dependency/unused-dependency analysis. Record CI-only gaps because Actions are
  unavailable.
- **Acceptance:** complete app builds; all feasible local checks pass; no known safe-to-fix
  regression remains; intentional exceptions list owner/removal condition; final report separates
  architecture, migration, deletion, and unrelated bug-fix commits.
- **Rollback point:** each focused cleanup/bug commit individually reversible; no opaque mega-commit.
- **Risks:** false-positive dead code through FFI/reflection/build tooling; verify reachability first.
- **Frontend:** full audit includes both.
- **Temporary compatibility:** none without new ADR and named removal packet.

## Execution order

`P0 -> P1 -> P2 -> P3 -> P4 -> P5 -> P6 -> P7 -> P8 -> (P9, P10) -> P11 -> P12 ->
(P13, P14) -> P15 -> P16`.

P9 and P10 may run in parallel only after P8 freezes shared evidence. P13 and P14 may run in
parallel only after P12 leaves one runtime path. P15 may begin after P6 but cannot ship before P12;
the listed order minimizes simultaneous migration surfaces.

## Decision log

- 2026-07-18: Plan created from clean `main` tips `maverick@1dc4f10`,
  `maverick-connectors@dfb351d`, and read-only `whoop-rs@375af9c`. Android release JKS was identified
  by filename only and excluded from connector signing.
- 2026-07-18: ABI boundary widened from decode trait to event/action because real WHOOP connection
  and historical flows require ordered transport, timers, cancellation, and persistence barriers.
- 2026-07-18: At plan time, `wasmi` was prototype-gated. Apple Guideline 2.5.2 remains release risk;
  artifact format and iOS publisher policy are deliberately separate.
- 2026-07-18: WC-P0 selected pinned `wasmi` 1.1.0 with `extra-checks`. Deterministic fuel, memory,
  malformed-module, custom-section, and realtime/history parity probes passed; all four Rust mobile
  static-library targets compiled. The 8 KiB development gates are 250 microseconds cold mean and
  warm p95, 40 MiB harness RSS, and 10 MiB static-archive delta. Full Xcode/device measurements and
  Apple review evidence remain named release gates; the disposable probe was deleted.
- 2026-07-18: WC-P1 added the `mav-connector-abi` leaf with no Maverick dependencies and pinned
  `minicbor` 2.2.2. Ascending integer-key maps plus canonical decode/re-encode equality reject
  duplicate, unknown, indefinite, unordered, non-shortest, and float encodings. All 27 event and 19
  action variants, artifact records, bounds, packed pointers, CDDL sources, byte vectors, and four
  schema hashes are frozen. SHA-256 exists only as a test dependency that checks schema source bytes;
  runtime hashing and trust remain WC-P2-owned.
- 2026-07-18: WC-P2 added non-instantiating artifact inspection and trust verification with pinned
  `wasmparser` 0.253.0, `sha2` 0.11.0, `ed25519-dalek` 3.0.0, and `subtle` 2.6.1. Validation enforces
  the four ordered terminal sections, canonical CBOR, size/count bounds, fixture hashes, canonical
  signature self-exclusion, publisher policy, key windows/rotation/revocation/scope, and strict
  Ed25519 verification. ADR-018 reserves Connector error codes 11000-11999; all 18 failure classes
  journal round-trip. Frozen digest, malicious corpus, truncation, and byte-flip tests pass without
  module instantiation.
- 2026-07-18: WC-P3 added the device-neutral `mav-connector-sdk`, exact native `TestDriver`, bounded
  action builder, ABI allocation/export glue, canonical metadata macro, and a zero-import Rust
  template that compiles reproducibly for `wasm32-unknown-unknown`. `mav-connector-tool` supplies
  pack digest/finalize, inspect, validate, and structural fixture CLIs using the host parser/runtime;
  finalization accepts public key/signature bytes from an external signer and self-verifies output.
  The connector repository now has an exact-version SDK consumer workspace, schema registry, and
  deep format/Clippy/test/Wasm/deterministic-package validator with no committed Maverick path.
- 2026-07-18: WC-P4 entered exact pinned `wasmi` 1.1.0 into the production runtime and froze the
  closed `mobile-v1` module, memory, table, recursion/value-stack, input/output/state, and
  five-million-fuel limits.
  Tests first failed on the absent instance API, then covered forbidden imports/features/start,
  malformed exports, pointers, output bombs, fuel, recursion, growth, module bounds, canonical ABI,
  fixture mismatch, and isolation. A separately signed Rust SDK template exposed that LLVM's MVP
  `call_indirect` encoding needs Wasmi's reference-types parser and more than 10,000 fixture fuel;
  preflight still rejects actual reference types, and both repository templates now declare the
  exact mutable-global/sign-extension/bulk-memory features with one-million fixture fuel. The signed
  artifact then ran its fixture successfully. The 8 KiB regression measured 15 microseconds cold
  mean and 2 microseconds warm p95; a direct runtime test harness used 4,587,520 bytes maximum RSS.
  Release runtime crates compiled for both Apple and both Android targets. Their rlibs measured
  1,858,360, 1,858,856, 1,653,732, and 1,675,360 bytes respectively; these are not a replacement for
  P0's full static-archive delta, which remains the linked-size gate until product integration.
- 2026-07-18: WC-P5 added the device-neutral `ConnectorHost` session beside the temporary compiled
  `HostRuntime`. It owns lifecycle, canonical event sequencing and trace hash, cancellation
  generations, guest-to-host operation/deadline id mapping, result correlation, manifest capability
  and characteristic enforcement, bounded atomic action queuing, timers, and session-local state
  staging. Tests drive scan→advertise→connect→pair→discover→subscribe→stream→disconnect entirely as
  data, reject wrong order and undeclared actions, prove queue atomicity, journal and ignore late
  cancelled results, and freeze trace hash `09b6ce81d8da683f`. Emitted fixed-point samples validate a
  closed stream/unit mapping and traverse SQI, timeline, provenance, and transactional storage before
  a later write enters the native queue. Durable namespaced connector state remains WC-P6-owned; the
  old compiled host remains only as the named WC-P12 compatibility path.
- 2026-07-18: WC-P6 added the independent forward-only `mav-connector-store` schema and core APIs
  for inspect, install, list, activate, migrate, rollback, remove, and trust enforcement. Approval
  tokens bind bytes, safe source provenance, trust/revocation revisions, and expiry, and are issued
  and consumed exactly once; installation repeats signature and embedded-fixture checks before one
  atomic transaction. Content-addressed artifacts retain manifest/source/trust/test provenance and
  append-only audit rows. State uses the
  exact connector/publisher/device/schema namespace with a 64 KiB digest-checked bound; publisher or
  schema changes require atomic migration, and rollback or active-update removal restores the prior
  snapshot. Tests cover restart recovery, stale approvals, verification/self-test/activation
  failure, downgrade refusal, four interrupted transaction boundaries, namespace isolation,
  migration failure/success, delete/quarantine, rollback restoration, key rotation, and revocation.
- 2026-07-18: WC-P7 exposed byte/source inspect and install, one-time approval tokens, installed
  records, activate/rollback/remove/trust operations, verified session open, generic transport
  events/actions, cancellation, draining, and lifecycle reports through UniFFI. `MavRuntime` keeps
  the old manifest method as an explicit WC-P12 migration seam while separately serializing the
  connector repository and one active P5 host. Rust tests cover exact high-byte payloads, stale
  tokens, safe structured errors, concurrent calls, every management transition, trust disable,
  cancellation generation, and ignored late results. Actual host-library generation produced both
  Swift `Data` and Kotlin `ByteArray` bindings with the complete device-neutral surface; CI now pins
  those symbols. Generated Swift parsed with the installed compiler; full iOS compilation was
  unavailable because `xcode-select` points at Command Line Tools, while Android static compilation
  was unavailable because no SDK is configured (the installed JDK is 25, not the required 17).
- 2026-07-18: WC-P8 added the dependency-free, no-std `whoop-protocol` reference crate in
  `maverick-connectors@4651c52`. Native and wasm32-compiled tests pin both frame generations, all
  three CRCs, command responses, metadata boundaries, the real eight-byte history cursor, safe
  generation-specific offload commands, real `whoop-rs` record routing, explicit unmapped records,
  malformed inputs, and rejection of destructive opcode 25. Its evidence matrix preserves every
  existing confidence tag; no Maverick protocol-ledger fact changed.
- 2026-07-18: WC-P9 added `dev.maverick.whoop4` in
  `maverick-connectors@4c2c51d`. Its gen4-only public-SDK state machine proves unbonded identity,
  subscription and hello ordering, safe historical retry/ACK, cancellation, disconnect/restore,
  standard/custom realtime, events, and every admitted v5/v7/v9/v12/v24/v25 record. Eleven
  embedded fixtures pass natively and in the signed Wasm artifact; deterministic rebuild,
  signature reconstruction, validation, and the existing public installer all activated the exact
  artifact successfully. Only the public test key and detached signature remain; the temporary
  private signer was deleted. The legacy path remains authoritative until WC-P12 parity proof.
- 2026-07-18: WC-P10 added paired `dev.maverick.whoop5` for WHOOP 5.0/MG in
  `maverick-connectors@a5e5be6`. Its generation-local state machine proves both service identities,
  pairing before discovery, the confirmed gen5 hello and R22 configuration sequence, safe history
  cursor ACK/retry, cancellation, reconnect/restore, standard/custom realtime, events, real v18 and
  v26 records, and bounded synthetic v20/v21 deep buffers. Nine embedded fixtures pass natively and
  in the signed Wasm artifact; deterministic rebuild, signature reconstruction, validation, and the
  public installer activated the exact artifact. Deep-stream unlock and MG calibration remain
  explicitly unverified rather than inferred. Only the public test key and detached signature
  remain; the temporary private signer was deleted. The legacy path remains authoritative until
  WC-P12 parity proof.
- 2026-07-18: WC-P11 expanded the signed artifacts in
  `maverick-connectors@1158ce6` to fourteen gen4 and twelve gen5 cases. Native execution and the
  no-JIT Wasm runtime now agree exactly on canonical input, ordered action, emitted-sample, and
  final-state hashes across admitted records, realtime/events, history cursor retry, restart, and
  malformed frames. Per-call maxima are 89,074 fuel/1,179,648 bytes linear memory for gen4 and
  3,631,187 fuel/1,245,184 bytes for gen5, within `mobile-v1`. On the local M1 host, full-artifact
  cold means were 2,756 and 2,548 microseconds and warm p95 was 27 and 29 microseconds. The full
  200–245 KiB artifacts exceed P0's explicitly 8 KiB-only 250-microsecond cold probe number; this
  size-dependent difference is recorded rather than normalized away. Rust regenerates both frozen
  reports from exact signed fixture bytes; Swift and Kotlin pin their schema, ids, hashes, flow
  coverage, and ceilings. Swift source parsing passed. Full iOS execution is unavailable without an
  Xcode simulator SDK; Android execution is unavailable without SDK/NDK and JDK 17. Both remain CI
  gates. Temporary signers were deleted; native and artifact paths remain dual only in tests until
  WC-P12.
- 2026-07-21: WC-P12 removed the complete compiled WHOOP crate, workspace/edge dependencies,
  manifest registry and per-device KV plugin surface, factory/runtime selector, manifest-only FFI,
  and compiled replay adapter. `mav-codec` now admits only declarative layouts and open standards;
  `mav-replay` verifies signed bytes and executes embedded fixtures through the production Wasm
  interpreter. Fresh runtimes list zero connectors, both committed WHOOP artifacts activate only
  after byte install, and the device-neutral template remains installable. A permanent stale-
  architecture gate, Cargo graph inspection, and built symbol/name inspection prove no proprietary
  connector is linked. Decode codes 3004–3006 remain numerically reserved under the ADR-017
  amendment but their obsolete names are retired. The sibling deletion is
  `maverick-connectors@a510c91`; both full Rust/doc/dependency and SDK/Wasm/package gates passed.
