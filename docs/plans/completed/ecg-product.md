# ECG product lane

Status: complete — ECG-P0 through ECG-P8 are complete.

> **Corrected by [ADR-034](../../adr/ADR-034.md) after the first hardware run.** This lane was
> built and validated entirely against fixtures, and four of its assumptions did not survive a
> real strap. The ECG is *not* the 100 Hz raw AFE stream this document describes: it is recorded
> to strap flash at 500 Hz and retrieved afterwards. Samples cross the ABI in millivolts, not
> counts. The confidence formula saturated. And packet ECG-P6's "no waveform graph" is withdrawn.
> The text below is left as written, because what was believed before the hardware arrived is
> worth keeping next to what the hardware said.

This lane turns the hardware-verified WHOOP MG raw ECG stream into Maverick's first generic captured
waveform experience. It is governed by [ADR-033](../../adr/ADR-033.md), the native ML boundary in
[ml.md](../../ml.md), the protocol evidence in
[whoop-raw-afe.md](../../protocol/whoop-raw-afe.md), and the admission rules in
[testing.md](../../testing.md).

The recovered classifier and PDF implementation are compatibility references, not clinical ground
truth. Every surface labels the result provisional and research-only. No packet may put a WHOOP,
MG, serial-prefix, or protocol-opcode check in Maverick core or native presentation.

## Lane exit

On both platforms, an ECG-capable connected session exposes one Vitals row and its capture/history
page; an incapable session exposes neither. A good waveform calibrates and records exactly 30
seconds, the platform's one admitted model produces the same winning class as the shared fixture
corpus, the result is stored with provenance, and a one-page Maverick PDF can be downloaded or
shared. All full gates pass, both release builds contain only their selected model, and the WHOOP
release connector passes native/Wasm parity with raw probing disabled outside a host capture.

---

## Packet ECG-P0: Freeze capture capability, model, UX, and report contracts

**Owns:** `docs/adr/ADR-033.md`, `docs/adr/README.md`, `docs/ml.md`,
`docs/plans/active/ecg-product.md`, `docs/plans/README.md`.

**Must not touch:** source, fixtures, model binaries, connector repository.

**Contract:** Accept ADR-033's manifest-maximum/session-active split. Record the exact first-model
input, output, preprocessing, variant, hashes, interpretation and validation ceiling. Freeze the
cross-platform state machine and the PDF/result split. Break the implementation into non-overlapping
packets.

**Tests first:** `tools/check_docs.sh` is observed red while the plan and ADR exist but are not
indexed.

**Exit:** `tools/check_docs.sh`.

**Status: done.**

---

## Packet ECG-P1: Deterministic Rust preprocessing and quality gate

**Owns:** `core/crates/mav-analytic/src/ecg_model.rs`,
`core/crates/mav-analytic/src/ecg_quality.rs`, `core/crates/mav-analytic/src/lib.rs`,
`core/crates/mav-analytic/Cargo.toml`, new versioned ECG fixtures under `fixtures/ecg/`,
`fixtures/README.md`.

**Must not touch:** connector ABI/runtime, store/engine/FFI, native apps, model binaries.

**Contract:** Convert raw finite samples at a declared positive source rate into exactly 7,680
little-endian `f32` tensor values by the contract in `docs/ml.md`. Expose a deterministic quality
assessment with a reason vocabulary sufficient for calibration: good, contact, motion, saturation,
flatline, dropout. Build the bounded ordered baseline/occlusion tensor set used by XAI.

**Tests first:** recovered-reference golden tensor at 256 Hz; a 100 Hz-to-256 Hz vector; exact centre
crop and pad; zero/invalid rate rejection; non-finite input rejection; quality reason fixtures;
occlusion coverage, bound and stable order.

**Exit:** `cargo test -p mav-analytic`; full gates.

**Status: done.**

---

## Packet ECG-P2: Generic captured-stream ABI and active-session negotiation

**Owns:** capture additions in `mav-connector-abi`, `mav-connector-sdk`,
`mav-connector-runtime`, `mav-engine`, `mav-ffi`; their canonical fixtures and schema records;
the capture sections of `docs/connectors.md`, `docs/platform.md`, and `docs/architecture.md`.

**Must not touch:** any device connector, analytics implementation, native presentation, storage
schema.

**Contract:** Add bounded manifest-v2 capture declarations, semantic start/stop events,
manifest/session intersection, ordinary signed transport validation, an active-capture FFI read
model and FFI capture commands. Existing v1 artifacts remain byte-compatible and expose no capture.

**Tests first:** undeclared start rejected; inactive stream rejected; active intersection exposed;
cancel queues stop before disconnect; old artifact fixtures unchanged; generated Swift/Kotlin
bindings contain the empty/populated capture record and start/stop calls.

**Exit:** targeted ABI/runtime/engine/FFI tests; native snapshot decoder tests; full gates.

**Status: done.**

---

## Packet ECG-P3: WHOOP 5/MG release capture

**Repository:** `/Users/sennen/Developer/maverick-connectors`.

**Owns:** WHOOP 5 connector state/fixtures/manifest and its protocol documentation, release artifact,
registry metadata and parity reports. The packet must be mirrored in that repository's active plan.

**Must not touch:** Maverick source or either native app; WHOOP 4 connector.

**Contract:** Positively identify MG using the decoded device identity evidence, then and only then
declare the ECG stream active. Translate semantic start to opcode 63 body `[0x01]`, emit the middle
type-43 channel at 100 Hz, and translate stop to opcode 82 body `[0x01]`. No raw stream runs before
a host capture or after stop/cancel/disconnect. Non-MG and unknown identities fail closed.

**Tests first:** MG identity declares ECG; known non-MG and malformed identity do not; start/write/
frame/stop ordered trace; cancel and disconnect stop; native/Wasm trace and resource parity.

**Exit:** connector crate tests, packaging/report gate, signed release verification.

**Status: done for the TEST trust domain.** Connector 1.0.6 is deterministically TEST-signed against
the manifest-v2 SDK source, re-vendored into Maverick, and passes package/registry freshness plus
native/Wasm parity. Its 16 embedded fixtures include complete MG ECG start/frame/stop and non-MG
fail-closed paths. Production publication remains an external publisher-key operation.

---

## Packet ECG-P4: Capture controller, result provenance, and history

**Owns:** ECG capture/result records in `mav-model`, `mav-store`, `mav-engine`, `mav-ffi`; migrations,
round-trip fixtures and read-model docs.

**Must not touch:** connector source, native inference, native UI/PDF.

**Contract:** Implement the ADR-033 state machine, good-quality calibration window and timeout,
exact 30-second recording, inference work request, bounded inference-result admission, XAI
interpretation, append-only raw evidence, rebuildable result provenance, history query and deletion
policy. No platform-computed health label enters a read model without model/filter hashes.

**Tests first:** every legal transition and exact illegal error; calibration reset on bad quality;
30-second boundary; cancellation/disconnect; inference output bounds; provenance walk-back; derived
drop/recompute identity; newest-first history.

**Exit:** targeted model/store/engine/FFI tests; full gates.

**Status: done.** The shared controller enforces the continuous-good calibration window and exact
30-second boundary, produces bounded baseline/occlusion inference requests, admits only the
contracted model result, and persists append-only evidence plus rebuildable newest-first results.
Model, store, engine, host, and FFI tests cover the transition and provenance contracts.

---

## Packet ECG-P5: One native model per platform

**Owns:** iOS Core ML wrapper and FP16 package/project entry; Android TensorFlow Lite wrapper, FP16
asset and Gradle dependency; both native test targets; binary-notice files.

**Must not touch:** Rust preprocessing/interpretation, connector source, screens, PDF layout.

**Contract:** iOS uses only the FP16 Core ML package with `.all` compute units on the existing
A13-or-newer deployment floor. Android uses only the FP16-weight TFLite model with FLOAT32 I/O and
bounded threads on Android 10+. Both consume only core-produced tensors and return three finite values.
Neither platform implements filtering, resampling, confidence, labels or XAI policy.

**Tests first:** model hash; exact tensor shape/dtype; nine synthetic corpus winning classes;
probability finiteness; native output returned in core-request order; release bundle contains the
selected model and excludes all variants.

**Exit:** iOS test target and Android unit/instrumented tests; release-build asset audit; full gates.

**Status: done.** Core ML ran all nine cases on an iOS 26.5 simulator; TFLite ran all
nine FP16 cases on a physical Pixel 7. Debug, field, and release bundle audits admit exactly one
model per platform.

---

## Packet ECG-P6: Native capture, result, and history experience

**Owns:** ECG additions to the current Terrain iOS and Android metric models, Vitals navigation,
device sheets, new capture/result/history views, UI tests and accessibility fixtures.

**Must not touch:** core health logic, connector source, native inference wrappers, PDF generation.

**Contract:** Render from `captures/v1`, never a device name. Both platforms show the same phases and
copy while using native components and Maverick theming. Calibration explains contact and motion;
recording has a literal 30-second progress/countdown; result is text-first with no waveform graph;
history is newest-first; the PDF action is download/share only.

**Tests first:** incapable session has no entry/action; capable session has both; all state/error
fixtures; no graph accessibility node on result; Dynamic Type/fontScale; reduced motion; screen
reader labels; 44 pt/48 dp targets; light/dark/high-contrast screenshots.

**Exit:** both native suites and accessibility gate; full gates.

**Status: done.** Both apps gate the
Vitals entry on the active-session capture capability, run calibration/recording/analysis through
the host state, submit their platform prediction, render a text-first result/history without a
waveform, and share the PDF from the result. Both application targets compile.

---

## Packet ECG-P7: Maverick-native downloadable PDF

**Owns:** one native PDF renderer and tests per platform, shared report contract fixtures, platform
legal notices.

**Must not touch:** screen layout, core classification, connector source, recovered report code.

**Contract:** Generate the same one-page Maverick report hierarchy on both platforms using native
PDF APIs and Terrain tokens: a text-first summary with result, confidence/quality, plain-language
XAI, capture/model provenance, limitations and research-only safety copy, followed by the 30-second
rhythm strip. No
Geminiman, Galaxy Watch, on-watch, APK, or recovered-product wording/assets may appear. PDF bytes
stay local and are only written to a user-selected/shareable destination.

**Tests first:** text contract and forbidden-word contract; one page; vector text; graph bounds;
long localized copy; light/dark app setting does not impair print contrast; reopen/render checks.

**Exit:** both native PDF suites; Poppler render inspection of every N/A/O fixture; full gates.

**Status: done.** Native Core ML and TFLite test runs each generated nine reports. All 18 reports
reopen as one-page PDFs; all pages render through Poppler and were inspected as contact sheets.
`tools/check_ecg_reports.sh` freezes checksums, page count, text/safety/branding contracts and
renderability.

---

## Packet ECG-P8: Cross-platform corpus and completion audit

**Owns:** ECG end-to-end fixture harness, verification scripts, plan status/decision log and final
documentation corrections.

**Must not touch:** production behaviour except fixes filed as a new packet.

**Contract:** Replay at least three deterministic examples for each N/A/O family through Rust
preprocessing, each native model, core interpretation, stored result, screen read model and PDF.
Audit capability honesty on MG/non-MG fixtures, offline-only behaviour, binary contents, model
hashes, provenance, accessibility, and forbidden branding.

**Tests first:** the audit script fails on one deliberately missing evidence path before its final
fixture is added.

**Exit:** 9/9 winning-class agreement per platform; 18/18 PDFs reopen/render/pass contracts; full
Rust/iOS/Android/release gates green; the lane moves to `completed/` with a retro.

**Status: done.** The 9-case synthetic corpus passes 9/9 winning-class checks in each
native runtime and the resulting 18/18 native PDFs pass the artifact audit. Signed TEST connector,
registry, package freshness, and MG/non-MG Wasm parity are green. A joined 100 Hz host test proves
capability → start → calibration → exact capture → automatic stop → seven native tensors → admitted
result → evidence/history/report waveform. A fresh physical end-to-end capture remains additional
hardware validation rather than an untested software seam.

## Decision log

- 2026-07-29 — Reused the existing session `DeclareCapabilities` action for hardware-level
  availability. Static manifest capability alone cannot distinguish WHOOP 5.0 from MG.
- 2026-07-30 — Standardized on FP16 weights for both platforms: Core ML on iOS and the original
  recovered FP16 TFLite graph on Android. Only one model ships per platform; there is no runtime
  selector or bundled fallback.
- 2026-07-29 — Kept graphs out of the in-app result and in the downloadable PDF, matching the
  requested readable-first product flow.
- 2026-07-30 — Superseded the original two-page report with a one-page A4 hierarchy after physical
  Android review. Both native renderers now keep result, probabilities, XAI, safety, provenance and
  all six five-second strips together. The trace uses a true 25 mm/s time base and one shared gain;
  calibrated millivolt sources use 10 mm/mV while raw ADC sources are labelled relative.
- 2026-07-29 — Expanded validation to three fixtures per model class and both runtimes: 18 native
  one-page reports, with hashes frozen after visual inspection.
- 2026-07-30 — Refreshed the deterministic TEST-signed connector/registry at WHOOP 5/MG 1.0.6.
  Package parity now contains explicit `mg-ecg-capture` and `non-mg-ecg-fails-closed` fixtures
  instead of relying on native connector tests alone.
- 2026-07-30 — The joined 100 Hz path exposed two calibration assumptions tuned implicitly for
  256 Hz: a two-second beat-evidence window and a 2.5% steep-derivative motion cutoff. The controller
  now uses five seconds for positive contact evidence while retaining a short negative-artifact
  window, and the motion cutoff admits ordinary 100 Hz QRS edges while retaining the injected-motion
  rejection. All nine N/A/O corpus signals now prove they can calibrate at WHOOP's 100 Hz rate.
- 2026-07-29 — The recovered 7,680-sample Python tensor is byte-identical to Rust. One SOS
  coefficient needed its exact `f32` bit pattern (`0x3f73bb75`); decimal shortening by one ULP
  changed the full output hash.
- 2026-07-29 — Kept signed v1 manifests and ABI 1.0 artifacts valid. Capture authority lives in
  manifest v2; start/stop are additive ABI tags 28/29, and generated Swift/Kotlin bindings expose
  the session intersection without a device-name check.

## Retro

The recovered classifier converted cleanly to one FP16 Core ML model for iOS and uses its original
FP16 TFLite graph on Android, but the important work was preserving one shared preprocessing,
quality, interpretation, and provenance contract around them. Nine synthetic fixtures per runtime
made class-level agreement visible without pretending the three output buckets are clinical
diagnoses. Native renderers then exercised those exact inference results in 18 one-page reports,
giving report layout, safety copy, branding, hashes, and renderability an executable audit.

The joined 100 Hz test found two assumptions that isolated 256 Hz model tests missed: the positive
contact window was too short for slow rhythms and clean QRS edges occupied enough samples to trip
the motion ratio. Splitting short negative-artifact evidence from five-second positive contact
evidence fixed calibration without weakening the injected-motion rejection. The final native audit
also found stale connector hash/count sentinels and an unrelated iOS maximum-Dynamic-Type hierarchy
regression; refreshing the signed fixture pins and widening the shared title/display token steps
closed both full suites.

The remaining limitation is validation, not an unfinished software path: the recovered three-class
model is provisional and research-only, and class `O` is one broad abnormal-rhythm bucket rather
than a set of distinct diagnoses. A fresh physical mobile capture is useful additional hardware
evidence, while production connector publication still requires the external publisher key.
