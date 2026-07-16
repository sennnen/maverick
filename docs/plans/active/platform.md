# Platform lane — native shells, UI migration, and release builds

This lane turns the shared core into two real applications without rebuilding the old NOOP
internals around it. It runs beside the numbered data milestones because the apps should be
integrated as soon as a core read model exists, while individual cards become available only when
their underlying analytics are admitted.

The visual and interaction specification is the current Aura shell in the NOOP workspace: four
hubs (`Today`, `Recovery`, `Strain`, `Sleep`), settings opened from every hub rather than occupying a
tab, calm crossfades, horizontal hub switching, and the existing card, chart, spacing, colour, and
type system. We preserve that product work. We do not copy NOOP's repositories, BLE clients,
analytics, persistence, ML wrappers, notification machinery, or giant platform view models. Those
are the implementation being replaced.

The product must remain honest while the migration is incomplete. A card backed by an unavailable
Maverick analytic shows the reason from the core. It never reads an old NOOP score, computes a
platform substitute, or fills the space with synthetic data. Empty and unavailable states are
normal product states, not temporary lies.

The lane exits when both applications build from clean checkouts, link the same core, render the
same fixture hashes in platform tests, present the migrated four-hub shell from a small typed app
store, and produce signed release-candidate artifacts from GitHub Actions when the required signing
secrets are present.

---

## Packet PL-P1: Freeze the host read-model contract

**Owns:** a new platform contract section in `docs/architecture.md`, `docs/testing.md`,
`apps/ios/README.md`, `apps/android/README.md`, and an ADR if the FFI surface must become stateful.

**Must not touch:** native UI, connector manifests, BLE code, or analytics formulas.

**Contract:** Write the exact boundary the native apps consume before either app is built. The host
must receive:

- core version and schema version;
- current connection and capture state;
- current heart rate and device summary;
- admitted analytics plus structured availability reasons;
- historical-sync progress and stable failure code;
- an immutable day snapshot suitable for the four hubs;
- an error-report summary with no raw secrets or cursor bytes.

Every returned read model has canonical JSON and a stable hash. Swift and Kotlin decode that JSON
into platform presentation structs; they do not recreate capability negotiation or analytic rules.
Commands cross the boundary as narrow named operations, never as arbitrary JSON RPC. The contract
states which calls may block, which thread owns the runtime, and how backpressure is reported.

**Tests first:**

- every canonical read model has an exact frozen JSON fixture and hash;
- unavailable analytics preserve their exact reason and missing requirements;
- platform decoders reject missing required fields and unknown schema versions;
- redacted fields never appear in status or report JSON;
- old fixtures remain readable for the supported compatibility window.

**Exit:** one document defines the complete app/core seam and both platform READMEs point to it.

**Status: done.** The boundary is frozen in `docs/platform.md`. ADR-013 keeps the stateless fixture
runner and adds one serialized stateful runtime for product use. The contract fixes construction,
connector registration, transport events and actions, canonical `host-snapshot/v1`, historical
cursor redaction, threading, error shape, native presentation limits, and compatibility tests before
the object is implemented.

---

## Packet PL-P2: Stateful host runtime over UniFFI

**Owns:** the host-runtime module in `mav-engine`, its narrow facade in `mav-ffi`, exact fixtures,
and tests. A frozen `mav-model` change requires an ADR before code.

**Must not touch:** native projects, BLE APIs, connector repositories, or UI.

**Contract:** Add a thread-safe UniFFI object that owns one engine instance. Native code can:

1. create it with an app-private database path and injected timezone;
2. register a validated connector package;
3. begin and end a device session;
4. feed notification bytes and command responses;
5. acknowledge native transport failures;
6. query immutable host snapshots;
7. drain bounded transport commands and user-reportable errors.

The runtime owns pipeline ordering. Native callers cannot invoke SQI, timeline, storage, features,
or analytics out of order. Input queues are bounded. Overflow becomes a typed error and never a
silent drop. All calls are deterministic under an injected clock in tests. No operation exposes a
database handle or raw acknowledgement cursor.

The existing stateless `run_capture` stays as the parity and fixture surface. Product code uses the
stateful object; it does not replay a growing JSON capture on every notification.

**Tests first:**

- a realtime capture fed chunk by chunk produces the frozen M1 snapshot and analytics hashes;
- splitting the same bytes at every possible boundary produces the same result;
- queue overflow returns a stable error and preserves already accepted input;
- reconnect starts a new session without losing prior stored samples;
- duplicate notifications remain idempotent through timeline and store;
- all public methods reject invalid state transitions;
- generated Swift and Kotlin bindings contain every intended method and no internal stage type.

**Exit:** one in-memory host-runtime test drives bytes to read model, plus one SQLite-backed restart
test proves state survives process death.

**Status: pending.**

---

## Packet PL-P3: Build and package the Rust core for both apps

**Owns:** reproducible build scripts under `tools/platform/`, generated-package ignore rules,
platform READMEs, and CI smoke jobs.

**Must not touch:** UI or connector behaviour.

**Contract:** Produce:

- an iOS XCFramework containing device and simulator slices, generated Swift bindings, header, and
  module map;
- Android shared libraries for every shipped ABI, generated Kotlin bindings, and the exact JNA
  dependency required by UniFFI.

Generated artifacts live under ignored build directories and are never hand-edited. Scripts pin
Rust targets, NDK/Xcode expectations, output layout, and checksums. A clean rebuild replaces the
whole generated package atomically so stale slices cannot survive.

**Tests first:**

- Swift compile test imports the generated module and calls `coreVersion`;
- Kotlin/JVM compile test compiles generated bindings;
- iOS simulator and Android emulator tests call `runCapture` and assert both frozen hashes;
- build scripts fail when a required architecture or generated file is absent;
- two clean builds from the same commit produce the same file inventory.

**Exit:** M0 platform linking and M1-P11 parity are complete on CI.

**Status: pending.**

---

## Packet PL-P4: Create minimal native application projects

**Owns:** project/build files, app entry points, base resources, platform test targets, and no
feature screens beyond a diagnostic launch surface.

**Must not touch:** NOOP source files in place or copy its old app model.

**Contract:** Create `Maverick` iOS and Android applications with new bundle/application ids,
Maverick naming, current supported OS floors chosen from actual toolchain support, strict warnings,
and no onboarding. Each app launches into a diagnostic surface that shows:

- app version and commit;
- core version;
- database open status;
- fixture parity status in debug builds only;
- latest structured startup error.

No generated sample metrics ship in release builds. Debug fixture actions are visibly labelled and
compiled out of release.

**Tests first:** cold launch, database-open failure, schema mismatch, missing core artifact, and
fixture parity.

**Exit:** both clean projects build and launch before any Aura screen is migrated.

**Status: pending.**

---

## Packet PL-P5: Native presentation stores and unavailable-state rules

**Owns:** one small app store per platform, typed presentation models, JSON decoders, preferences
needed by the migrated shell, and their tests.

**Must not touch:** Rust analytic logic, BLE transport, or visual component files.

**Contract:** The platform store adapts immutable core read models into view state. It may format
dates, units, and localized strings. It may not derive a health score, infer availability, merge
days, repair missing data, or query SQLite directly.

Each field has one of four explicit states:

- `value` — admitted value plus provenance/quality summary;
- `collecting` — required input is currently being gathered;
- `unavailable` — structured core reason;
- `failed` — stable error code plus safe user message.

Stale snapshots carry their age and remain visibly stale. Platform refresh is a single serialized
operation. Settings are local presentation preferences only unless a setting maps to a documented
runtime command.

**Tests first:** exact JSON decoding, schema rejection, every availability state, stale-data label,
unit conversion, locale formatting, refresh coalescing, and error redaction.

**Exit:** screens can depend only on the platform store and pure presentation models.

**Status: pending.**

---

## Packet PL-P6: Migrate the Aura design system and four-hub shell

**Owns:** native design tokens, reusable components, tab shell, settings presentation shell, and
visual regression fixtures. It may copy and rewrite the corresponding Aura files from NOOP.

**Must not touch:** old NOOP data models, repositories, BLE clients, analytics, onboarding, coach,
journal, backup, HealthKit, Health Connect, widgets, notifications, or connector-specific screens.

**Contract:** Preserve the existing product specification:

- hubs: `Today`, `Recovery`, `Strain`, `Sleep`;
- settings opened by the common hub header, never a fifth tab;
- horizontal decisive flick between hubs, vertical scrolling wins;
- calm 240 ms transition using the existing cubic easing;
- current spacing, type roles, card shapes, chart language, colour families, reduced-motion and
  reduced-transparency behaviour;
- iOS floating glass capsule and Android platform-appropriate docked navigation using the same
  information architecture, not pixel-for-pixel imitation.

Rewrite components that are entangled with old data. Copy only code whose behaviour is visual and
whose dependencies fit the new project. Remove duplicate files and numbered copies rather than
choosing one by accident.

At this packet's end, all four hubs render honest empty/unavailable states from the platform store.
No old score is present.

**Tests first:** tab selection/reselection, swipe thresholds, settings presentation, accessibility
labels, large text, reduced motion, light/dark appearance where supported, and screenshots at the
agreed phone sizes.

**Exit:** both apps show the cleaned Aura shell and compile without any NOOP domain package.

**Status: pending.**

---

## Packet PL-P7: Plumb realtime and admitted analytics into the shell

**Owns:** the four hub screens, live-heart-rate sheet, small metric detail views, and platform UI
tests.

**Must not touch:** core formulas or recreate missing scores.

**Contract:** Bind existing Maverick outputs first:

- realtime heart rate, session summary, device name, battery, and connection state;
- PRV metrics and their quality/provenance;
- structured Recovery unavailability;
- historical-sync progress once M5-P7 lands.

Recovery, Strain, and Sleep visual slots remain in the preserved information architecture, but show
the core reason until M4/M6 admit their values. The copy explains what is missing without blaming
the user or claiming a subscription will fix it. A card becomes numeric only through a new core
fixture and contract version.

**Tests first:** live HR updates, disconnect/stale states, provisional PRV label, unavailable
Recovery, no-value leakage into Strain/Sleep, screen parity against the same read-model fixture, and
snapshot hashes displayed only in diagnostics.

**Exit:** changing a core fixture changes the corresponding values on both platforms through the
same seam; no UI test seeds a platform database.

**Status: pending.**

---

## Packet PL-P8: Connector import and built-in standard heart-rate transport

**Owns:** connector package validation/import UI, native connector registry adapters, standard BLE
Heart Rate transport, and docs split between the core and `maverick-connectors`.

**Must not touch:** proprietary connector contents inside the app repo or add runtime code loading.

**Contract:** Maverick ships only a built-in standards connector for BLE Heart Rate Service
`0x180D`/measurement `0x2A37` plus standard battery/device-information services where available.
WHOOP and future proprietary connectors remain in the private, separate connector repository.

An imported connector is a signed/versioned data package that conforms to the core manifest schema.
Native app code validates identity, schema range, declared capabilities, hashes, and required GATT
permissions before registration. iOS does not dynamically load arbitrary native code; connector
logic that cannot be declarative must be compiled into a reviewed core extension at build time and
versioned separately. The UI must say when a connector requires an app rebuild.

Tests use the standards-defined heart-rate payload and checked-in connector fixtures. They do not
invent a fake wearable family.

**Tests first:** valid package import, tamper rejection, unsupported schema, duplicate version,
downgrade refusal, capability display, standard 8/16-bit HR and RR decode, permission denial,
disconnect, and package removal without deleting user data.

**Exit:** a real standard BLE HR sensor can feed the core; a WHOOP package can be validated and
registered without being committed to this repository.

**Status: pending.**

---

## Packet PL-P9: Platform error reporting and diagnostics

**Owns:** user-facing error surfaces, diagnostics screens, export/share plumbing, and tests.

**Must not touch:** raw protocol secrets or connector-private capture files.

**Contract:** Every failure exposed by the runtime maps to:

- concise user message;
- stable `MAV-` code;
- next safe action;
- optional diagnostic detail;
- report-bundle entry with app/core/connector versions and redacted recent events.

Persistent failures remain visible until resolved or dismissed. Transient reconnect noise is
coalesced. Export never includes raw health streams by default; explicit full-data export is a
separate action with a clear confirmation.

**Tests first:** code-to-copy mapping, repeated-error coalescing, redaction, failed export, report
size bounds, connector/version inclusion, and accessibility announcement.

**Exit:** a user can report a failed connection or decode without developer tools, and the report
contains enough versioned evidence to reproduce it.

**Status: pending.**

---

## Packet PL-P10: Signed release-candidate workflows

**Owns:** GitHub Actions workflows, release build scripts, version stamping, signing documentation,
and artifact verification.

**Must not touch:** product behaviour.

**Contract:** Every push to `main` runs core/docs/parity tests, builds release variants, and uploads
private workflow artifacts:

- `Maverick.ipa`, signed with the configured Apple distribution identity and provisioning profile;
- `Maverick.apk`, signed with the configured Android release keystore;
- checksums, build manifest, commit SHA, core version, connector schema range, and test reports.

Pull requests build unsigned simulator/emulator artifacts and run tests, but never receive release
signing secrets. Main-branch release jobs fail clearly when required secrets are missing; they do
not relabel unsigned output as a release candidate. Artifact retention is bounded. Workflows use
pinned action revisions where practical, minimal permissions, dependency caches keyed by lockfiles,
and concurrency cancellation for superseded branch builds.

Required secrets and their exact formats are documented. iOS signing, export method, bundle id, and
team id are one coherent configuration. Android signing verifies the final APK with `apksigner`.
The iOS job verifies the archive, exported IPA contents, entitlements, signature, bundle id, and
embedded version. Both install on a clean simulator/emulator before upload where the platform
permits it.

**Tests first:**

- workflow syntax and local script unit tests;
- debug/PR build with no secrets;
- intentional missing-secret failure on a protected test path;
- APK signature and install verification;
- IPA bundle, entitlement, and signature verification;
- artifact manifest hashes every uploaded file;
- version/build number match the commit and workflow run.

**Exit:** one pushed commit produces verified private `.ipa` and `.apk` release-candidate artifacts
after signing secrets are configured.

**Status: pending.**

---

## Execution order

The safe order is:

1. PL-P1 and PL-P2: contract and runtime.
2. PL-P3 and PL-P4: package the core and establish clean app projects.
3. PL-P5 and PL-P6: presentation boundary, then visual shell.
4. PL-P7: real values into the shell.
5. PL-P8 and PL-P9: connector installation and user-grade failure handling.
6. PL-P10: signed release automation after local release builds pass.

M5-P2 through M5-P7 may proceed beside this lane. M4 and M6 can add values to existing unavailable
slots after PL-P7 without redesigning the shell. Onboarding remains out of scope until the connector
install and permission flow is real enough to explain truthfully.
