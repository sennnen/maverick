# Milestone plans

Each milestone gets one file here with its full work-packet breakdown: the packets an agent picks up one at a time, each with its owned files, frozen contract, tests-first list, and exit commands. The packet template and the execution protocol are defined in `docs/PLAN.md` and `skills/work-packet`.

The lifecycle is a move between two directories. A milestone in flight lives in `active/`, and agents update packet statuses and the decision log in place as packets land. When the milestone's exit criterion is demonstrably true, the file moves to `completed/` and gains a short retro section: what surprised us, what drifted from the plan as written, and which doc fixes were filed as a result. Nothing is deleted; a completed plan is the record of what was actually done, drift included. `tools/check_docs.sh` fails if a plan file exists in either directory without being listed below.

Milestones are gated by exit criteria, not dates. The full table with scope and exit lines is in `docs/PLAN.md`.

| # | Name | One line | Packets |
|---|---|---|---|
| M0 | Bedrock | Workspace, CI gates, docs system, frozen `mav-model`, complete `mav-frame`, errors and observability, UniFFI hello world | [active/M0.md](active/M0.md) |
| M1 | First vertical slice | Realtime HR from a WHOOP capture, end to end, identical snapshot hash on both platforms | [active/M1.md](active/M1.md) |
| M2 | Connector framing hardening | An adversarial format exposed the closed enum; ADR-012 made framing manifest data | [completed/M2.md](completed/M2.md) |
| M3 | RR variability and honest availability | Published time-domain variability; PPG labelled PRV; no invented Recovery score | [completed/M3.md](completed/M3.md) |
| M4 | Sleep | Gravity/HR/RR/respiration features, rule-based staging first, night-summary snapshots | not yet broken into packets |
| M5 | Historical sync | Backfill state machine, all known record versions, clock correction, plausibility gates, recompute triggers | not yet broken into packets |
| M6 | Analytics breadth and ML | Strain and the remaining metrics that pass the admission rule; native inference wired with golden vectors | not yet broken into packets |
| M7 | Cloud connector | A manifest of kind `cloud` through the same pipeline, proving the abstraction covers non-BLE sources | not yet broken into packets |
| M8 | Hardening | Observability and fixture-coverage audits, error-report UX, doc gardening, ADR backfill | not yet broken into packets |

One item stands outside the sequence. The hardware epoch is the standing checklist that activates when the physical straps arrive: every code-inferred protocol fact gets verified or corrected against live captures, ledger tags flip to hardware-verified, and fixtures are regenerated from real captures. It has no fixed place in the milestone order because it starts on a delivery date we do not control; it lives in `docs/protocol/whoop.md`, not here.
