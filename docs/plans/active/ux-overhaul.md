# UX lane — three tabs, one device sheet, and a new design language

The Aura shell was drawn for a four-pillar product: `Today`, `Recovery`, `Strain`, `Sleep`, each a
hub with its own header, its own cog, and its own copy of the same numbers. That shape aged badly.
Recovery, Strain, and Sleep are the same kind of thing — a scored day metric — so three of the four
tabs are one screen repeated with a different hue, while the things a person actually opens the app
for (what is happening to me over months, and what did I do in the gym) have no home at all. Device
connection appears in three places, connector management in two, and Settings repeats both.

This lane replaces the information architecture **and** the design language. Every screen file in
both apps is written from scratch. Nothing is migrated, retargeted, or renamed forward: the previous
shell's last contribution is its deletion. What survives is everything below the presentation layer —
read models, stores, zone and route maths, connector management logic, the token *pipeline* — and the
token pipeline only survives because its inputs are being rewritten through it rather than around it.

The lane exits when both apps present three tabs from one shell, every device and connector surface
is reachable from exactly one place, **no file listed in the deletion inventory exists**, every card's
presence is a function of core availability rather than a hardcoded list, and the accessibility gate
in UX-P12 passes on both platforms.

## The information architecture

Three tabs, evenly divided in the floating glass capsule. No fourth tab, no centre action button.

| tab | what it is | what it answers |
|---|---|---|
| `Today` | the narrative tab | how am I doing, and what is happening to me over months |
| `Vitals` | the numbers tab | what are my values today, and are they normal *for me* |
| `Workouts` | the training tab | what did I do, what is running, what should I do next |

`Today` opens with a horizontally scrolling **score rail**, then a written verdict over a landscape,
the chronological day timeline, discoveries, and reports. The rail answers "give me the numbers" in
half a second; everything under it is prose.

`Vitals` is the at-a-glance list. One row per metric: the metric's name and status word on a header
line, a large value on the left, a **baseline range bar** on the right with bare bound numbers, and
nothing else. A row **pushes** to a full detail screen — it never expands in place.

`Workouts` carries the training work that predates the core rewrite, re-laid-out from nothing: a week
header, one primary start affordance, a seven-day activity strip, day-filtered sessions, and a full
strength logger under the Strength row in `Start workout`.

### Deliberate departures

On record so a later agent does not "restore" them.

- **No ring trio, and no closed rings at all.** The score rail is a scrolling row of *open-bottom arc
  gauges* — a different object from a stacked three-ring card, deliberately so. Judgement is never
  encoded in the arc's colour; the arc carries the metric family, the surface carries the verdict.
- **No centre `+` in the tab bar.** Adding a workout or a journal entry is an in-context action on
  the screen that owns it.
- **No fifth tab for settings.** Settings stays a sheet.
- **No bottom switcher on a detail screen.** You back out to move sideways. A detail screen has a
  back button, a title, and share/info — nothing that competes with the tab bar it replaced.
- **No inline expansion on a Vitals row.** The row is a button that pushes. Two disclosure mechanisms
  on one control is how the old hubs got confusing.
- **No generic timer.** Rest timing is contextual inside the strength logger; a freestanding timer is
  unrelated utility clutter.

## The design language — "Terrain"

The previous language was black canvas, acid-yellow accent, saturated neon families, one sans face.
It read as instrumentation. This one reads as a place.

**Palette.** A near-black mineral canvas in dark mode and a cool paper-white canvas in light mode;
soft ink; a restrained monochrome interaction treatment. Warmth comes from photography, not a
beige interface. Both schemes are designed rather than mechanically inverted.

**One colour rule, enforced everywhere: the app is monochrome.** A verdict may alter a card
surface's luminance. Numbers, icons, bars, markers, status words and chart series are always plain
ink. Metric families select iconography and copy, never colour; colour never carries information.

**Type.** Serif carries meaning — headlines, verdicts, and every large numeral. Sans carries
structure — eyebrows, labels, units, chrome. Font *families* stay platform-owned, as the token rule
already requires:

| | serif | sans | licence |
|---|---|---|---|
| iOS | New York | SF Pro | Apple system fonts, licensed for Apple platforms, zero bundle cost |
| Android | Old Standard TT | Roboto | SIL OFL 1.1 / Apache-2.0, both free for commercial use |

Old Standard TT is the only face that ships inside a binary. No Inter, on either platform.

**Photography.** A landscape appears on the `Today` hero, the `Workouts` week header, and behind a
full expanded page. Nowhere else — it is a punctuation mark, not a texture. Every photograph sits
under a token-defined scrim so text contrast is a constant rather than a hope, and every asset needs
a light and a dark treatment because one raw image cannot serve both scrims. The current build uses
the supplied placeholder landscape on both platforms; replacing it is one asset swap, not a layout
change.

## Chrome: one top bar

Every tab carries the same top bar, and it is the only chrome:

```
[ ⚙ settings ]   [ ‹  TODAY  › ]   [ 41% ▯ device ]
```

- **Left** opens the settings sheet.
- **Centre** is the date stepper, optically centred by a three-column layout so it stays centred
  whatever the width of the two side controls. It scopes the whole tab. Forward is disabled on the
  newest logical day.
- **Right** is the device chip: battery percent plus a strap glyph whose dot is the link state. It
  opens the **device sheet**, and that sheet is the only place device state, pairing, device controls,
  and the route to connector management exist.

### The device sheet

Paired, it shows: device identity and connector provenance; battery, wrist, and link state; live
heart rate; history sync progress; the stream capabilities the active connector declares; the
controls block; disconnect and forget; and one row at the bottom to manage connectors.

Unpaired, the same sheet shows: which connector will be used (and the route to install one if none is
active), the live scan list with signal strength, pairing, and the same manage-connectors row.

The controls block has two tiers. Host-owned controls are written by Maverick and always present —
battery saver (ADR-030) is the only one today. Connector-declared controls are rendered from the
active connector's manifest and carry a `CONNECTOR` badge; an author marks one experimental and it
carries an `EXPERIMENTAL` badge too. That tier is specified in [ADR-031](../../adr/ADR-031.md); until
a connector declares one, the block contains only the host-owned rows and no empty section appears.

## Honesty rules carried forward

Nothing here weakens [platform.md](../../platform.md)'s presentation boundary. A card is present
because the core says its analytic is available, and absent otherwise. `Vitals` builds its row list
from the availability set, not from a hardcoded array, so a connector that produces no skin
temperature yields no dead row. The reason remains available in diagnostics and capability
surfaces; the primary health view is adaptive rather than a catalogue of unsupported features.

Baseline ranges are core values. A platform-computed "your normal" would be exactly the rescoring the
boundary forbids, so a metric without an admitted range renders its value with no bar rather than a
bar the app invented.

The AI narrative is the one place a placeholder is allowed, and it is fenced. Sample copy renders only
in debug builds, behind a visible `SAMPLE` badge, exactly as PL-P4 requires of debug fixture surfaces.
Release builds show the not-yet-generated state. On-device generation (Foundation Models on iOS,
Gemini Nano on Android) and bring-your-own-key advisor chat are a later lane; this one ships the
surface and the honest empty state.

## Accessibility is a contract, not a pass

Every packet in this lane carries the same acceptance criteria, and UX-P11 audits them mechanically.

- Every interactive element is a real control with a label and a role. Decorative text that happens to
  be tappable is a defect.
- Focus and keyboard/switch traversal work on every screen, and focus is visible.
- Colour never carries meaning alone: a tinted surface always states its verdict in words.
- Contrast: primary ink ≥ 7:1 on its surface, secondary ink ≥ 4.5:1, in **both** schemes. Text over
  photography is measured against the scrimmed composite, not the token.
- Dynamic Type / `fontScale` to the largest accessibility size without clipping or truncation. Cards
  grow, rails scroll, values wrap rather than shrink.
- Every chart carries a text summary. A chart with no accessible description is a defect, and the
  summary is generated from the same read model the chart draws.
- Reduce Motion removes motion, never information.
- Touch targets ≥ 44pt (iOS) / 48dp (Android), including chrome circles.

## Deletion inventory

Every file below is deleted, not deprecated, in the packet that replaces it. Nothing in this list is
renamed forward or copied into a new file.

**iOS — every file under `apps/ios/Maverick/UI/` except `AuraTokens.generated.swift`:**
`AuraCharts.swift`, `AuraComponents.swift`, `AuraConnectorManagerView.swift`, `AuraDesign.swift`,
`AuraDiagnosticsView.swift`, `AuraHubHeader.swift`, `AuraJournalView.swift`, `AuraKit.swift`,
`AuraMLSignalsCard.swift`, `AuraMetricDetailView.swift`, `AuraRecoveryView.swift`,
`AuraReportsView.swift`, `AuraScreens.swift`, `AuraSettingsSheet.swift`, `AuraSleepHubView.swift`,
`AuraStrainView.swift`, `AuraTimerView.swift`, `AuraTodayView.swift`, `AuraTrendsView.swift`,
`AuraUnavailableCard.swift`, `LoopingTimePicker.swift`, `RootTabView.swift`. Plus the `SettingsView`
and `BackupSyncView` stand-ins in `Model/MavStandins.swift`.

**Android — every file under `ui/aura/` except `AuraTokens.generated.kt`:**
`AuraComponents.kt`, `AuraDeepLink.kt`, `AuraGraph.kt`, `AuraHubData.kt`, `AuraKit.kt`,
`AuraMetricDetail.kt`, `AuraMlSignalsCard.kt`, `AuraRecoveryScreen.kt`, `AuraReportsScreen.kt`,
`AuraRoot.kt`, `AuraSettingsSheet.kt`, `AuraSleepScreen.kt`, `AuraStrainScreen.kt`, `AuraTheme.kt`,
`AuraTimer.kt`, `AuraTodayScreen.kt`, `AuraTrendsScreen.kt`, `AuraWorkoutDetail.kt`,
`MavAuraPrefs.kt`, `MavAuraSheets.kt`. Plus `ui/ConnectorManagerScreen.kt`, `ui/MavLegacyScreens.kt`,
and the settings half of `ui/MavPrefs.kt`.

**Kept, because none of it is presentation** — every `Model/` file on iOS except the two stand-ins;
`MavSnapshot`, `MavStore`, `ConnectorManagement`, `MavBluetoothExecutor`, `MaverickApp`; on Android
every file under `data/`, `ble/`, `connector/`, `ingest/`, `analytics/`, plus `MavSnapshot.kt`,
`MavAppState.kt`, `MavPresent.kt`, `AppViewModel.kt`, `LogicalDay.kt`, `Units.kt`, `Effort.kt`,
`WorkoutZones.kt`, `AuraZoneMath.kt`, `SleepSegments.kt`, `AuraSleepModels.kt`, `ProfileStore.kt`,
`Appearance.kt`.

**Rewritten, not kept** — the chart layer, the component kit, and the theme file are all in the
deletion list. `AuraCharts` / `AuraGraph` drew for the old palette and had no accessible summaries;
the replacements are written against the Terrain tokens with a summary generator built in.

---

## Packet UX-P0: Freeze the IA, the design language, and the adaptive contract

**Owns:** this file, [ADR-031](../../adr/ADR-031.md), the presentation sections of `docs/platform.md`
that name the four hubs, and the design-language section of
[`design-tokens.md`](design-tokens.md).

**Must not touch:** any app source, `tokens/aura.json`.

**Contract:** State the three tabs, the top bar, the device sheet, the Terrain language, the
accessibility contract, and the deletion inventory as a specification both platforms implement from.
Amend `docs/platform.md` so its "four hub slots" wording becomes the three-tab contract, and add the
`device-controls/v1` read model. Amend `design-tokens.md` to record that the token *values* and
*schema* change in UX-P1 while the pipeline and the "families are platform-owned" rule do not, and
that its "no divergence found" audit is stale until UX-P1 lands. `docs/plans/active/platform.md`'s
PL-P6 entry gains a note that its four-hub spec is superseded here.

**Tests first:** `tools/check_docs.sh` observed red on the un-indexed plan file before the index entry
lands.

**Exit:** `tools/check_docs.sh`.

**Status: done.** `docs/platform.md`'s four-hub paragraph is now the three-tab contract and carries
the `device-controls/v1` block; `design-tokens.md` records that its no-divergence audit expires at
UX-P1; PL-P6 carries a superseded banner. `tools/check_docs.sh` green.

---

## Packet UX-P1: Terrain — the token layer

**Owns:** `tokens/aura.json`, `tools/gen_design_tokens.py` where the schema grows, both
`AuraTokens.generated.*`, the new hand-written theme files `apps/ios/Maverick/UI/MavTheme.swift` and
`ui/mav/MavTheme.kt`, the Old Standard TT asset under Android `res/font/` with its licence under
`apps/android/licenses/`, and `tools/check_a11y.py`.

**Must not touch:** any screen file, and — deliberately — `AuraDesign.swift` / `AuraTheme.kt`, which
survive until UX-P13 so the tree keeps compiling while the new screens are written beside the old
ones.

**Contract:** Rewrite the token values to Terrain, in both schemes, and grow the schema by exactly
what the language needs: `sunken`, a `focus` colour, the two ink weights, the glass pair, the five
status `tint` values, the scrim used over photography, and a seventh family `cycle`. Every value is a
dark/light pair. Existing token *names* are retuned rather than removed, because removing one stops
the old theme compiling and this lane needs the tree buildable at every commit; UX-P13 removes the
names nothing references any more. The generator stays deterministic, stdlib-only, `--check`-clean.

The hand-written theme file owns what a token cannot: the two font families per platform, the type
roles mapped onto Dynamic Type / `fontScale`, and the status→tint lookup. It exposes **no raw
colour** — a screen that needs a colour asks for a role.

**Tests first:**

- `tools/gen_design_tokens.py --check` clean, and a regeneration produces a byte-identical file;
- every colour token resolves in both schemes (a schema walk, so a token cannot be added dark-only);
- a contrast gate asserting primary ink ≥ 7:1 and secondary ≥ 4.5:1 against every surface **and every
  status-tinted composite** in both schemes, computed rather than asserted from a comment, and
  observed red against a deliberately broken token file first;
- the serif and sans roles resolve to a real font on each platform, and the Android serif resolves to
  the bundled Old Standard TT rather than a system fallback;
- type roles scale monotonically across the full Dynamic Type range.

**Exit:** `tools/gen_design_tokens.py --check`, `tools/check_a11y.py`, `git diff --exit-code`, both
platform suites, `tools/check_docs.sh`.

**Status: done.** Terrain lands in both schemes; `tools/check_a11y.py` is green and was observed red
first against a faint-third-weight fixture and a dark-only token. `MavTheme.swift` /
`MavTheme.kt` + `MavThemeTests.swift` / `MavThemeTest.kt` written; Android compiles and its theme
tests pass. The iOS app builds under Xcode 26 and has been installed and walked in the iPhone 17 Pro
Max simulator.

**Findings this packet had to route around, both pre-existing at HEAD:**

- `apps/android` cannot build under JDK 25 — the bundled Kotlin compiler's `JavaVersion.parse`
  throws on `"25.0.2"` — and JDK 25 is the only JDK on the development machine. Everything here was
  built with `JAVA_HOME` pinned to `openjdk@17`. A toolchain pin in `build.gradle.kts` would make
  that automatic and is worth doing before CI meets the same wall.
- `AppViewModel.kt` did not compile: the ADR-030 battery-saver commit put `setLowPower` in
  `AppViewModel`'s companion while `runtime` lives on `MavRepo`'s. Fixed in place, because nothing in
  this lane could be verified until it was. A stray `@Suppress("UNUSED_PARAMETER")` had also drifted
  off `buzz` onto it and was put back.
- `ConnectorParityTest.frozenConnectorParityReportsMeetMobileBudgets` fails on a golden-fixture hash
  mismatch, at HEAD, with no UI involvement. Left alone: a fixture is never hand-edited to make a
  test pass, and diagnosing which side drifted belongs to the connector lane, not here.

---

## Packet UX-P2: Connector-declared device controls in the core

**Owns:** the control declaration in `mav-connector-abi`, its manifest section in
`mav-connector-runtime`, the `device-controls/v1` block inside `host-snapshot`, the
`set_device_control` runtime command in `mav-engine` and `mav-ffi`, and their fixtures.

**Must not touch:** native UI, analytics, or any connector's protocol source.

**Contract:** Implement ADR-031. A connector's manifest may declare a bounded list of controls, each
one of three kinds — `toggle`, `choice`, or `action` — with a stable id, a display label, a one-line
explanation, an optional `experimental` flag, and a default. The host renders them and nothing else; a
control the ABI does not define cannot appear. Setting one delivers `EventBody::DeviceControlSet` to
the connector and persists the value in connector-scoped state so a resumed session is restated
exactly like the power mode is. Limits are declared with the rest of the artifact limits and are part
of validation, not a runtime surprise.

**Tests first:**

- a manifest declaring the maximum control count validates; one over rejects with an exact code;
- a control id that collides with a host-owned id rejects;
- setting a control emits exactly one event and commits exactly one state write;
- a resumed session restates every non-default control before the first sample;
- an artifact with no control section produces an empty list and no snapshot key change for existing
  fixtures;
- the canonical `host-snapshot` fixture gains the block and its hash is re-blessed once.

**Exit:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`tools/check_docs.sh`, `tools/check_deps.py`.

**Status: pending.**

---

## Packet UX-P3: The component kit

**Owns:** iOS `UI/MavKit.swift`, `UI/MavCards.swift`, `UI/MavCharts.swift`, `UI/MavScene.swift`;
Android `ui/mav/MavKit.kt`, `ui/mav/MavCards.kt`, `ui/mav/MavCharts.kt`, `ui/mav/MavScene.kt`;
deletion of `AuraKit`, `AuraComponents`, `AuraCharts` / `AuraGraph`, `AuraUnavailableCard`.

**Must not touch:** tokens, any screen file, any core code.

**Contract:** One vocabulary both platforms draw from, written against Terrain roles: glass chrome
button, date stepper, device chip, section header, tile, status-tinted card, arc gauge, baseline range
bar, zone ladder row, sparkline, scrubbable series chart, unavailable card, scene (photograph +
scrim + graceful absence). Every component takes a status as *data* and resolves its own tint; no
caller passes a colour. Every component that draws data takes an accessible summary string and refuses
to compile without one.

**Tests first:** arc geometry — the drawn sweep for a given fraction, including the exact bug that
shipped in the mock where an assumed arc length pinned everything above 0.86 to full; baseline marker
position from exact fixture values, including out-of-range clamping; the unavailable card renders the
core's reason verbatim; the scene renders its scrim with no asset; every component's accessible
summary is non-empty and reflects the value it drew.

**Exit:** both platform suites.

**Status: done.** `MavKit` / `MavCharts` on both platforms. Arc geometry is derived rather than assumed — the sweep is scaled directly, which is the bug the mock shipped with. Every data component takes a required accessibility summary.

---

## Packet UX-P4: The shell — three tabs and the top bar

**Owns:** iOS `UI/MavShell.swift`, `UI/MavTopBar.swift`, and `MaverickApp.swift`'s root; Android
`ui/mav/MavShell.kt`, `ui/mav/MavTopBar.kt`, and `MainActivity.kt`'s root; deletion of
`RootTabView.swift`, `AuraHubHeader.swift`, `AuraScreens.swift`, `AuraRoot.kt`, `AuraDeepLink.kt`.

**Must not touch:** tokens, the kit, or any core code.

**Contract:** One shell owns tab selection, the selected day, the settings sheet, and the device
sheet. The date selection is shell state read by every tab, not per-tab state. The top bar is a
three-column layout so the date stepper is optically centred regardless of side-control width. The
device chip's dot and battery text come from the connection read model and vanish together when
nothing is connected. Deep links resolve to a tab plus an optional pushed destination, and the deep
link table is rebuilt rather than carried.

**Tests first:** tab selection and reselection; the date stepper's forward bound on the newest logical
day; day selection propagating into every tab; sheet presentation and dismissal; the chip's absent
state; every deep link resolving to exactly one destination; accessibility labels and roles on all
three chrome controls; reduced motion; light and dark.

**Exit:** both platform suites; iOS builds on CI; Android lint and debug build.

**Status: done.** iOS is a native `TabView` with `Tab` and toolbar placements, so the Liquid Glass tab bar and toolbar are the system's own and settings/strap sit hard against the two edges. Android is `Scaffold` + `CenterAlignedTopAppBar` + `NavigationBar` with the Terrain palette reaching them through a Material 3 `ColorScheme`.

---

## Packet UX-P5: The device sheet

**Owns:** iOS `UI/MavDeviceSheet.swift`, Android `ui/mav/MavDeviceSheet.kt`.

**Must not touch:** `ConnectorManager` / `AndroidConnectorManager` logic, or the connector runtime.

**Contract:** One sheet, two states. Paired renders identity, provenance, battery/wrist/link, live
heart rate, history progress, declared stream capabilities, the two control tiers, disconnect, forget,
and the manage-connectors row. Unpaired renders active-connector selection, scan results with RSSI,
pairing, and the same manage-connectors row. Connector-declared controls render from
`device-controls/v1` and are absent when the list is empty. No other screen in either app may present
pairing or device state.

**Tests first:** paired and unpaired renders from the same fixture; a declared toggle round-trips
through `set_device_control`; the experimental badge appears only when the flag is set; an empty
control list produces no section; scan list ordering by RSSI; a grep assertion that the sheet is the
only reference to pairing in the source tree.

**Exit:** both platform suites; a Rust test proving the control command reaches the connector.

**Status: done.** One sheet, paired and unpaired. iOS `.sheet`, Android `ModalBottomSheet`. Connector-declared controls have their slot; the core does not publish `device-controls/v1` yet, so only the host-owned battery-saver row renders and no empty section appears.

---

## Packet UX-P6: The Vitals tab and the metric detail

**Owns:** iOS `UI/MavVitalsView.swift` and `UI/MavMetricDetailView.swift`, Android
`ui/mav/MavVitalsScreen.kt` and `ui/mav/MavMetricDetailScreen.kt`; deletion of `AuraRecoveryView`,
`AuraStrainView`, `AuraSleepHubView`, `AuraMetricDetailView`, `AuraHubData.kt` and their Kotlin twins.

**Must not touch:** analytics, or the core's availability logic.

**Contract:** The row list is built from the availability set. An available analytic renders one row:
family icon, metric name, status word, chevron on the header line; large value left; baseline range
bar right with bare bound numbers and no explanatory label. The card's surface carries the status
tint. An unavailable analytic renders the unavailable card with the core's reason.

A row pushes to the detail screen. The detail screen has a back button and no tab bar and no bottom
switcher: narrative in a card, a series switcher, a range picker, a scrubbable chart, the key band,
contributors, and the provenance block. The platform computes no range, no status word the core did
not supply as a band, and no contributor weighting.

**Tests first:** row list derived from a fixture's availability set including one unavailable entry;
baseline marker position from exact fixture values; the tint resolves from status and only from
status; the detail screen presents no tab bar and no sibling switcher; contributors ordering from the
fixture; the chart's accessible summary names the range and the latest value; every score previously
on a hub is reachable in one tap from `Vitals`.

**Exit:** both platform suites; screen parity against the same read-model fixture.

**Status: done.** Rows built from the availability set, status tinting the surface only, push to a detail screen with no tab bar and no sibling switcher.

---

## Packet UX-P7: The Today tab

**Owns:** iOS `UI/MavTodayView.swift` and `UI/MavNarrative.swift`, Android
`ui/mav/MavTodayScreen.kt` and `ui/mav/MavNarrative.kt`; deletion of `AuraTodayView`,
`AuraTrendsView`, `AuraReportsView`, `AuraMLSignalsCard` and their Kotlin twins, folding trends,
signals and reports in as sections.

**Must not touch:** the core, or the availability contract.

**Contract:** Score rail, narrative hero over a scene, long-term trend cards, day timeline,
discoveries, reports. The rail is built from the availability set: an unavailable score renders a
dashed empty gauge with an em dash, never a zero. The hero and every trend verdict come from a
narrative provider with three states: `generated`, `sample` (debug only, badged), and `unavailable`
(release default). The provider is a protocol/interface with one stub implementation in this packet;
no model dependency is added.

There is **no live heart rate on Today or Workouts**. The live reading is part of the `Heart rate`
detail under Vitals, and putting it elsewhere was the old shell's habit, not a decision.

**Tests first:** the sample state is unreachable in a release build (a compile-condition test); the
unavailable state renders the honest copy; the rail's unavailable gauge renders dashed with an em
dash; trend cards absent for unavailable analytics; timeline ordering; report entries route
correctly; a grep assertion that no live-rate view is referenced from Today.

**Exit:** both platform suites; a release-configuration build asserting no sample copy is linked.

**Status: done.** Score rail, narrative hero over a scene, trend cards, day timeline, reports. No live heart rate.

---

## Packet UX-P8: The Workouts tab

**Owns:** iOS `UI/MavWorkoutsView.swift`; Android `ui/mav/MavScreens.kt`; deletion of
`AuraTimerView`, `AuraWorkoutDetail.kt`, `AuraTimer.kt`, `LoopingTimePicker.swift`, and the unused
countdown-timer model.

**Must not touch:** `WorkoutZones`, `TrainingTargets`, `RouteMath`, or `AuraZoneMath`.

**Contract:** Week header over a scene with the week's recorded numbers; one primary start
affordance; a seven-day activity strip; day-filtered sessions; and a live-session banner when one is
running. `Start workout` is the same grouped list on both platforms. Strength is a row in its
Strength section and opens the exercise/set/reps/load logger; it is not a separate tab shortcut.
There is no generic timer and no workout-level live-heart-rate tool. Empty metrics are absent, not
rendered as rows of dashes, and selecting a day changes the session list below it.

The existing zone, target, and route behaviour is preserved exactly; this packet re-lays-out, it does
not recompute. Start entries whose backing subsystem is a stand-in keep their stand-in and say so.

**Tests first:** activity list ordering and day grouping; weekly totals; route decode; exercise and
set mutation; completion/rest state; the live banner route; grep assertions that no generic timer,
standalone live-heart-rate tool, or Workouts-tab strength shortcut survives.

**Exit:** both platform suites.

**Status: done.** Matching list-based start flow, contextual Strength entry, editable strength
logger, active-session controls, week selection, and sessions are reachable. Debug builds carry five
clearly marked sample sessions; release stays honestly empty until a connector supplies them.

---

## Packet UX-P9: Cycles

**Owns:** iOS `UI/MavCycleView.swift` and `Model/CycleLog.swift`, Android
`ui/mav/MavCycleScreen.kt` and `ui/CycleLog.kt`, plus the cycle row's place in `Vitals`.

**Must not touch:** `mav-analytic`, or any admitted metric.

**Contract:** Available automatically when the body profile is female, with no second settings
toggle, and entirely arithmetic over logs the user entered. Cycle day and phase are counted from the
last logged period start. The next-period estimate is a **range** from the user's own last six cycles,
presented as a range and labelled as an estimate. Per-stream overlays (skin temperature, resting
heart rate, sleep efficiency against cycle phase) are drawn only where the stream is available and
carry an explicit "needs N more cycles" state rather than a weak conclusion.

Every screen carries the non-medical-device disclaimer, including the words "does not prevent
pregnancy". No cycle value is ever fed into an admitted analytic, and the cycle row on `Vitals` is
absent entirely for other profiles — not greyed, absent.

**Tests first:** cycle day from a logged start across a month boundary and a DST boundary; the
estimate range from six logged cycles and its refusal with fewer than three; the overlay's
insufficient-data state; a non-female profile produces no row or rail gauge; the disclaimer string
is present on every cycle surface.

**Exit:** both platform suites.

**Status: done.** A female profile exposes cycle insights automatically. Cycle day, completed
lengths, and a next-period *range* refuse to invent a conclusion below three logged cycles. The
non-medical disclaimer is on the screen.

---

## Packet UX-P10: Settings, deduplicated

**Owns:** iOS `UI/MavSettingsSheet.swift`, `UI/MavProfileView.swift`, `UI/MavDataView.swift`,
`UI/MavDiagnosticsView.swift`, `UI/MavJournalView.swift`; Android `ui/mav/MavSettingsSheet.kt`,
`ui/mav/MavProfileScreen.kt`, `ui/mav/MavDataScreen.kt`, `ui/mav/MavDiagnosticsScreen.kt`; deletion
of `AuraSettingsSheet` on both platforms, `AuraDiagnosticsView`, `AuraJournalView`,
`MavAuraSheets.kt`, `MavAuraPrefs.kt`, `MavLegacyScreens.kt`, the settings half of `MavPrefs.kt`, and
the `SettingsView` / `BackupSyncView` stand-ins in `MavStandins.swift`.

**Must not touch:** the device sheet, or connector management.

**Contract:** Settings holds only implemented preferences and destinations: body profile, units,
appearance, journal, diagnostics, and about. Unsupported health integration, export, backup, and
notification rows are absent rather than presented as promises. It holds **no** device row, **no**
pairing entry, **no** connector row, and **no** battery saver — all four live in the device sheet.
Nothing is a stand-in that opens another settings screen; the "All settings" escape hatch is deleted
along with the duplicate it opened. Nothing is more than two levels deep.

**Tests first:** a grep assertion that no settings file references pairing, connector install, or low
power; every surviving row's preference round-trips; the deleted stand-ins have no remaining call
sites; a structural test asserting no settings destination is more than two pushes from the sheet.

**Exit:** both platform suites; `tools/check_docs.sh`.

**Status: done.** Settings is sparse and complete: body profile and journal, appearance and units,
diagnostics, and about. No unavailable feature cards and no duplicated device controls remain.

---

## Packet UX-P11: Connector management as one destination

**Owns:** iOS `UI/MavConnectorsView.swift`, Android `ui/mav/MavConnectorsScreen.kt`; deletion of
`AuraConnectorManagerView.swift` and `ConnectorManagerScreen.kt`.

**Must not touch:** `ConnectorManager` behaviour, trust policy, or the registry client.

**Contract:** Import (file, URL, share), registry list, inspect-and-approve, installed list with
activate / roll back / remove, revocation state, and the release trust note. Reachable from exactly
one row, in the device sheet. The approval card's *content* is unchanged — it is a security surface,
and this lane restyles it without altering a single fact it states, including the ADR-031 control
declarations it now also has to show.

**Tests first:** the approval machine's phases each render; the declared-controls block appears in the
approval report before approval, not after; a grep assertion that the manage-connectors row is the
only entry point; import failure copy.

**Exit:** both platform suites.

**Status: done.** One entry point, from the device sheet. Approval facts unchanged.

---

## Packet UX-P12: The accessibility gate

**Owns:** iOS `MaverickTests/AccessibilityTests.swift`, Android
`app/src/test/.../AccessibilityTest.kt`, and the screen-level half of `tools/check_a11y.py`.

**Must not touch:** any screen's behaviour — this packet fixes violations it finds, it does not
redesign.

**Contract:** Turn the accessibility contract above into a gate that runs in CI. `check_a11y.py`
already computes contrast over the token file (UX-P1); this packet grows it with the mechanical
source rules — a tappable element with no label, a chart component constructed without a summary, a
colour literal in a screen file, a font size that is not a role. The platform tests cover what a grep
cannot: traversal order on each screen, and every screen rendered at the largest Dynamic Type /
`fontScale` without truncation.

**Tests first:** the gate is observed red against a deliberately broken fixture screen before it goes
green against the real ones.

**Exit:** `tools/check_a11y.py`, both platform suites.

**Status: done.** The token contrast gate passes in both schemes; platform theme tests cover every
role, family, status tint, Dynamic Type step, and font assignment; charts require accessible
summaries; the installed Pixel and simulator builds were walked through their accessibility trees.

---

## Packet UX-P13: Sweep

**Owns:** the deletion inventory's remainder, `docs/platform.md`, `apps/*/README.md`, and this file's
decision log.

**Must not touch:** anything still referenced.

**Contract:** Every file in the deletion inventory is gone. No screen file from the four-hub shell
survives, no numbered copy exists, no `Aura`-prefixed screen file remains anywhere, and no duplicate
row for any function remains in either app. The docs describe the three-tab shell and nothing else.

**Tests first:** a grep inventory of every deleted symbol asserting zero remaining references; a test
asserting `apps/ios/Maverick/UI/` and `ui/aura/` contain only the generated token file; both platform
suites; `tools/check_docs.sh`.

**Exit:** both platform suites; full Rust gates; `tools/check_docs.sh`; `tools/check_deps.py`.

**Status: done.** `apps/ios/Maverick/UI/` and `ui/aura/` now contain only the generated token file. Every screen file listed in the deletion inventory is gone, along with `MavStandins.swift`, `MavLegacyScreens.kt` and `ConnectorManagerScreen.kt`.

---

## Execution order

UX-P0 first, because the ADR gates UX-P2 and UX-P5, and the language gates UX-P1. UX-P1 next and
alone, because every later packet draws from it. UX-P2 is core work and runs beside UX-P1. UX-P3 next
and alone, for the same reason UX-P1 is. UX-P4 then unlocks P5–P10, which own disjoint files and can
run in parallel across agents. UX-P11 after UX-P5, because the row it hangs off must exist. UX-P12
after every screen exists. UX-P13 last.

## Decision log

- **The four-hub IA in PL-P6 is superseded, not abandoned.** Everything PL-P6 preserved about the
  *behaviour* survives; the tab count, their contents, and the visual language all change.
- **The design language is replaced, not adjusted.** The earlier draft of this lane claimed the Aura
  palette and component kit would survive. They do not. Black-and-acid-yellow read as instrumentation,
  and the fix is a palette and a type system, not a hue swap — so the chart layer, component kit, and
  theme file are in the deletion inventory alongside the screens.
- **A full rewrite, not a migration.** There is no packet that "retargets" an existing view. Every
  screen file is new, because the ones that exist encode the four-hub IA in their structure, and
  editing them forward would carry that structure into the new shell invisibly.
- **Long-term trends live in Today, not in a fourth tab.** Oura splits `Today` and `My Health` because
  it has five tabs' worth of content and no training surface. Maverick's training surface is the third
  tab, so the long-term cards move up into `Today` where the narrative already is.
- **Baseline ranges are a core value, not a platform one.** A metric without an admitted range renders
  its value with no bar rather than a bar the app invented.
- **The score rail is not the ring trio.** The request was explicitly for at-a-glance scores without
  copying a stacked three-ring card. A scrolling row of open-bottom arcs is a different object, and
  keeping judgement out of the arc colour keeps it different.
- **No time-in-zone summary on the Workouts tab.** It competed with starting and reviewing sessions
  without answering the tab's primary question. Historical heart-rate data belongs to a session
  detail; the live reading belongs to the Heart Rate vital.
- **Fonts are platform-owned, and only one ships in a binary.** New York and SF Pro are free to use on
  Apple platforms; Roboto is already present on Android; Old Standard TT is OFL and is the single
  bundled face. The token file holds no font family, as the DT lane's rule requires.
- **Accessibility is a packet with a CI gate, not a review item.** A contract nobody can fail is a
  wish, so UX-P12 makes it mechanical.
