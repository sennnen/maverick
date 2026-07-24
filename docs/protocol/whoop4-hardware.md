# WHOOP 4.0 (gen4) on live hardware — and why there is no raw AFE stream

Companion to `whoop-raw-afe.md` (which cracked the WHOOP MG's raw stream). This records what a **worn
WHOOP 4.0** actually does on the wire, driven from the standalone Android lab tool, and the headline
result: **gen4 has no raw analog-front-end stream to crack.** It computes everything on the strap and
emits processed records.

**Confidence:** `[HW]` — verified against a worn WHOOP 4.0, strap `WHOOP 4C2581565`, over Bluetooth
from the same standalone Android tool used for the MG (it auto-detects the generation from the
discovered service). Battery `SOC report: 66.03` at capture time.

---

## TL;DR

- The gen4 wire format is confirmed on hardware: a **4-byte frame header** `AA len16 crc8(len)`, the
  inner `[0x23, seq, opcode]` payload, and a `crc32` trailer — distinct from gen5's 8-byte header.
- The **9-byte hello** (`opcode 35`, body `[0u8; 9]`) is answered; an empty body draws silence.
- gen4 uses a **different BLE service** (`61080001-8d6d-82b8-614a-1c8cb0f8dcc6`) with characteristics
  `…0002` (command, write), `…0003` (command-response), `…0004` (events), `…0005` (data), `…0007`
  (console) — all notify except the command char.
- **There is no raw AFE stream.** `opcode 63` — the MG's raw trigger — is *rejected* here (status
  `0x02`). A curated safe-opcode sweep produced no high-rate stream and no packet type 43.
- What flows instead: **COMMAND_RESPONSE (36)**, **EVENT (48)**, **METADATA (49)**, **CONSOLE_LOGS
  (50)**, and **HISTORICAL_DATA (47)** on offload. Live HR is on the standard `180d/2a37`
  characteristic (WHOOP gates the subscription — a CCCD write there returned status 128).

## Why there is no raw stream — the strap does the DSP itself

The firmware console (packet type 50) streams its signal-processing state in the clear, and it shows
the analog front end being **reduced to features on the strap**, not shipped as samples:

```
SIGPROC-WEAR-DETECT V5: moving from state 3 to state 3
state_cnt = 7200, opt_ir_near_m = 0.058585, opt_ir_m_ratio = 1.194684, opt_amb_m = 0.000899
orient_60 = 0.0, orient_30 = 0.0, acc_x = -0.983489, acc_y = -0.030444, sleep = 0
lm_w = 0, lm_w_cnt = 0, lm_hr_pk_m = 0.0, lm_hr_pk_dev = 0.0, lm_harm_perc = 0.0
Analytics: Populating event id 0x20, EVENTPKT_...
SOC report: 66.03
```

Read that vocabulary: `opt_ir_near_m`, `opt_ir_m_ratio`, `opt_amb_m` are **means and ratios** of the
optical channels, already computed; `lm_hr_pk_m` / `lm_hr_pk_dev` / `lm_harm_perc` are the on-strap
**heart-rate peak detector**; `sleep`, `orient_*`, `acc_*`, and `SIGPROC-WEAR-DETECT` are on-strap
classifiers. The optical hardware family is the same as the MG's (IR + ambient are named here too),
but gen4 never surfaces the waveform — only the derived numbers, packaged into EVENT and HISTORICAL
records. The MG's raw AFE (opcode 63 → type 43, 100 Hz PPG/ECG + 25 Hz red/IR/ambient) is a **gen5
capability**; gen4 predates it.

## What this means for the platform

WHOOP 4.0 is a **processed-record device**, not a raw-signal device. In the capability-tier framing
(see `analytics.md`), it lands around **HR + RR-intervals + motion + skin-temp + processed SpO2** —
enough to drive HRV, sleep, stress, recovery and respiratory rate, but with **no raw PPG/ECG
waveform**, so raw-signal features (independent SpO2 from red/IR, ECG morphology, PPG-morphology BP,
beat-level AFib from the waveform) are simply **unavailable** on this device and must be reported as
such, never fabricated. The MG sits several tiers higher precisely because it streams the raw AFE.

## Where this lives in code

No new connector work follows from this: the `whoop4` connector already decodes the gen4 record set
(`Gen4V5` / `Gen4V24` / `Gen4V25`) into HR / RR / skin-temp / motion samples, which is exactly the
data gen4 exposes. There is no raw decoder to add because there is no raw stream. The lab tool that
produced these findings (gen4 framing, live characteristic discovery, the hello + offload flow) lives
in the standalone `whoop-mg-android` project, outside this repository.
