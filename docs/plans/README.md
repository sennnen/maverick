# Milestone plans

Each milestone gets one file here with its full work-packet breakdown: the packets an agent picks up one at a time, each with its owned files, frozen contract, tests-first list, and exit commands. The packet template and the execution protocol are defined in `docs/PLAN.md` and `skills/work-packet`.

The lifecycle is a move between two directories. A milestone in flight lives in `active/`, and agents update packet statuses and the decision log in place as packets land. When the milestone's exit criterion is demonstrably true, the file moves to `completed/` and gains a short retro section: what surprised us, what drifted from the plan as written, and which doc fixes were filed as a result. Nothing is deleted; a completed plan is the record of what was actually done, drift included. `tools/check_docs.sh` fails if a plan file exists in either directory without being listed below.

Milestones are gated by exit criteria, not dates. The full table with scope and exit lines is in `docs/PLAN.md`.

| # | Name | One line | Packets |
|---|---|---|---|
| M0 | Bedrock | Workspace, CI gates, docs system, frozen `mav-model`, complete `mav-frame`, errors and observability, UniFFI hello world | [completed/M0.md](completed/M0.md) |
| M1 | First vertical slice | Realtime HR from a WHOOP capture, end to end, identical snapshot hash on both platforms; connector contracts superseded by ADR-017 | [completed/M1.md](completed/M1.md) |
| M2 | Connector framing hardening | An adversarial format exposed the closed enum; ADR-012 made framing manifest data | [completed/M2.md](completed/M2.md) |
| M3 | RR variability and honest availability | Published time-domain variability; PPG labelled PRV; no invented Recovery score | [completed/M3.md](completed/M3.md) |
| M4 | Sleep | Gravity/HR/RR/respiration features, rule-based staging first, night-summary snapshots | not yet broken into packets |
| M5 | Historical sync | Safe backfill controller, admitted record versions, canonical merge, recompute triggers | [completed/M5.md](completed/M5.md) |
| M6 | Analytics breadth and ML | Strain and the remaining metrics that pass the admission rule; native inference wired with golden vectors | not yet broken into packets |
| M7 | Host-mediated cloud source | A manifest of kind `cloud` through the same pipeline, with native acquisition supplying bounded source events and connectors still getting no network | not yet broken into packets |
| M8 | Hardening | Observability and fixture-coverage audits, error-report UX, doc gardening, ADR backfill | not yet broken into packets |
| PL | Platform lane | Native apps, cleaned Aura shell, core plumbing, connector import, signed RC artifacts | [active/platform.md](active/platform.md) |
| WC | WebAssembly connectors | Signed runtime-loaded connectors, SDK, WHOOP migration, legacy deletion, import and final audit | [completed/wasm-connectors.md](completed/wasm-connectors.md) |
| WF | WHOOP fidelity | Protocol corrections against the whoop-rs oracle, ABI snapshot sentinel, release regeneration, connector CI | [active/whoop-fidelity.md](active/whoop-fidelity.md) |
| LP | Live path | Telemetry scoring, commit accounting, bounded dedup, ClockMap wiring, host decomposition, mav-obs | [active/live-path.md](active/live-path.md) |
| AS | Analytic spine | DailySnapshot pipeline over FFI, Kotlin analytics deletion, capability negotiation live, platform dedup | [active/analytic-spine.md](active/analytic-spine.md) |
| G | Gardening | Docs truth, dead directories and edges, parking mav-frame/mav-codec, store read-path consolidation | [active/gardening.md](active/gardening.md) |
| DT | Design tokens | One Aura tokens file generating Swift and Kotlin theme constants with a CI drift check | [active/design-tokens.md](active/design-tokens.md) |
| ECG | ECG discovery | Finding the MG's undecoded ECG waveform on the wire: the raw-data flag, packet type 43, and the probe build | [active/ecg-discovery.md](active/ecg-discovery.md) |
| UX | UX overhaul | Three tabs (Today, Vitals, Workouts), one device sheet, connector-declared controls, and the deletion of the four-hub shell | [active/ux-overhaul.md](active/ux-overhaul.md) |
| ECG-P | ECG product | Generic captured ECG, MG-only session gating, native inference, result history, and downloadable Maverick reports | [completed/ecg-product.md](completed/ecg-product.md) |

One series of packets has no plan file. The `WHOOP-P*` tags in `mav-analytic` source comments —
`WHOOP-P5`, `WHOOP-P6`, `WHOOP-P8` — name the analytics-porting work that admitted HR zones, VO2max,
readiness, SpO2, and the IMU features from the `[WRS]` oracle. It ran as a series of direct commits
rather than as a filed milestone, so the tags are provenance for where a formula came from, not
pointers to a document. They are kept because the citation is real; no plan file will be
back-filled.

One item stands outside the sequence. The hardware epoch is the standing checklist that activates when the physical straps arrive: every code-inferred protocol fact gets verified or corrected against live captures, ledger tags flip to hardware-verified, and fixtures are regenerated from real captures. It has no fixed place in the milestone order because it starts on a delivery date we do not control; it lives in `docs/protocol/whoop.md`, not here.
