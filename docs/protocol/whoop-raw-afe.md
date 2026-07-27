# WHOOP raw AFE stream (ECG + PPG) — gen4 and gen5

The raw analog-front-end (AFE) stream carries the WHOOP MG's single-lead **ECG** and the **optical
PPG** channels straight off the sensor, undecimated. No public source had decoded it; the docs, this
project's own connector, and the `whoop-rs` oracle all chased the wrong command. This document is the
result of cracking it on live hardware.

**Both generations have it.** The body of this document describes gen5 (5.0 / MG); WHOOP 4.0 answers
the same trigger with a different channel layout and no ECG — see *WHOOP 4.0 (gen4) also has a raw
AFE stream* below.

**Confidence:** `[HW]` — verified against a worn WHOOP MG, strap `MGB0261172`, firmware **50.33.2.0**,
and separately against a worn WHOOP 4.0, over Bluetooth from a standalone Android tool bonded to each
strap. Captures and the tool live outside this repo (`whoop-mg-android/`); a memory note records the
paths.

---

## TL;DR

- **Start the stream with opcode `63` and a `[0x01]` revision byte.** Not `START_RAW_DATA` (81).
- 81 is *accepted* (status OK) but streams nothing. `enable_raw_data_w_ecg` is **not a config key on
  this firmware** and is always rejected. Both were red herrings the whole ecosystem believed.
- The stream is packet type **43 `REALTIME_RAW_DATA`**, two interleaved subtypes, **one pair per
  second**.
- Subtype **v0x0a** (1920 B) carries **three 100-sample `u16` channels at 100 Hz**: PPG, **ECG**, PPG.
- Subtype **v0x0b** (1924 B) carries **three 25-sample signed `i32` channels at 25 Hz** — the pulse-ox
  triad **red + IR + ambient** (identified with light controls; see below).
- The stream is **wear-gated**: it stops ~2–3 s after the strap decides it is off-wrist.
- Stop with opcode `82` (`[0x01]`), or just drop the link.

## The command

```
opcode 63, body [0x01]   →  COMMAND_RESPONSE status 1 (OK), stream begins
```

The `[0x01]` is a revision byte, the same requirement that gates the config key exchange (`117`/`118`)
and that made the old empty-body `START_RAW_DATA` fail with `Raw data start unsupported revision: 0`
on the firmware console. Only revision `1` is accepted (revision `2` → rejected). Extra body bytes
past the revision are ignored.

Opcode 63 sits next to `GET_AFE_PARAMETERS` (62) in the opcode map — it was found by a targeted probe
of stream/AFE-adjacent opcodes, reading each response, after the config-key and START_RAW_DATA paths
were exhausted.

## Frame format

Every frame is a normal gen5 envelope (`AA 01 … CRC`) whose decoded payload starts with the packet
type. Two subtypes alternate, sharing a per-second timestamp:

| Byte | Field | Notes |
|------|-------|-------|
| 0 | packet type | `0x2b` = 43 `REALTIME_RAW_DATA` |
| 1 | subtype/version | `0x0a` (1920 B) or `0x0b` (1924 B) |
| 3 | frame counter | +1 per frame of that subtype |
| 7..10 | Unix time (u32 LE) | **seconds**; +1 per v0a/v0b **pair** ⇒ one pair per second |

### Subtype v0x0a — the decoded one (100 Hz)

Three contiguous **100-sample little-endian `u16`** channels at fixed byte offsets in the payload:

| Channel | Offset | Baseline (worn) | Identity |
|---------|--------|-----------------|----------|
| PPG A | `0x055` | ~470 | optical — green HR PPG (inferred; see *Green and yellow* below) |
| **ECG** | `0x11d` | ~1220 | **single-lead electrode** |
| PPG B | `0x1e5` | ~3900 | optical — green HR PPG (inferred) |

100 samples per one-second frame ⇒ **100 Hz** per channel. Sample *i* is at
`unix_time * 1000 + i * 10` ms. After the channels come two ~25-sample `u32` blocks (≈25 Hz derived
values); the rest of the frame is zero padding.

**How the ECG channel was identified — the electrode-contact control.** With a finger on the metal
electrode the middle channel (`0x11d`) is a stable ~1220 baseline carrying the biopotential; with the
electrode *floating* the same channel rails between the ADC extremes (0 ↔ ~4000) and its variance
explodes (σ ≈ 475 vs 99). The two optical channels barely move either way — they only need skin
proximity, not the electrode. That flip *is* the proof of which channel is the ECG lead.

### Subtype v0x0b — the pulse-ox triad (25 Hz)

Three **25-sample signed little-endian `i32`** channels at fixed offsets in the otherwise
zero-padded 1924 B payload. 25 samples per one-second frame ⇒ **25 Hz** each. They are exactly the
channels a reflectance pulse oximeter needs — two illuminating LEDs plus an ambient reference:

| Channel | Offset | On-skin baseline | Identity | Confidence |
|---------|--------|------------------|----------|------------|
| A | `0x026` | ~+250k | **RED PPG** (~660 nm) | `[HW]` |
| B | `0x0ee` | ~+180k | **IR PPG** (~940 nm) | `[HW]` |
| C | `0x6b9` | ~+300k | **ambient-light reference** | `[HW]` |

The samples are **signed**. On skin they sit positive; off skin, channels A and B rail to a
**negative** floor (~−100k), so a decoder must read `i32`, not `u32`. (An earlier draft of this
document called them `u32` — that was wrong.)

**How the three were told apart — two lighting controls on a worn strap.**

- *Reflective LED vs ambient (finger-press / air-dip).* Pressed to skin, A and B carry a positive,
  pulsatile PPG and are highly correlated (r ≈ 0.95); lifted into open air they both rail to ~−100k
  (an ambient-subtracted reading over-subtracts when there is no LED backscatter). C does the
  opposite — low when skin blocks the room, then **floods to tens of millions in open air** — the
  signature of an ambient-light photodiode, not a reflective channel.
- *Red vs IR (940 nm remote).* Firing a TV remote (≈940 nm) at the sensor moves **B about five times
  more than A** (Δ ≈ +170k vs +32k; correlation with the ambient channel 0.60 vs 0.44). B's band
  passes 940 nm and A's rejects it, so **B is IR and A is red**. Broadband C spikes on the remote too.

So v0x0b is the **SpO2 / pulse-ox measurement set** — red + IR + an ambient reference, sampled slow
and full-resolution, complementing v0x0a's 100 Hz HR/ECG path. Turning the raw counts into an SpO2
percentage is a downstream calibration problem (it needs a reference oximeter), not a wire problem.

### Wear-gating — the raw stream only runs when worn

The strap **stops emitting the raw AFE stream within ~2–3 s of deciding it is off-wrist.** Fully
removing it kills the stream after roughly one frame, so there is no way to capture a sustained
off-skin trace by simply taking it off. The lab tool copes with a watchdog that re-fires opcode 63
whenever frames stall, plus keeping the sensor lightly touched (a finger, or the wrist) so brief
lifts ride the 2–3 s grace window. This gate is why the optical-identity controls above had to be run
as quick press/lift cycles rather than a clean on/off.

### Green and yellow — what the evidence does and does not show

The firmware console names four optical sources — `red`, `green`, `opt_ir`, `opt_amb` — and **no
`yellow`**. v0x0b accounts for red, IR and ambient, so **green is the remaining LED, and it lives in
the v0x0a 100 Hz path**: green is the standard motion-robust heart-rate wavelength and 100 Hz is the
HR rate, and v0x0a's optical channels do saturate under bright external light, confirming they are
light-sensitive. That placement is a strong inference, not a direct wavelength measurement — an
attempt to isolate green by pressing the sensor to a colour-cycling screen was confounded (pressing
floods the ambient-subtracted channels so they rail regardless of colour, the strap's own LEDs
reflect off the glass, and wear-gating cut the capture off after the green frame). Pinning the
wavelength would need a *directional* green source, which the available kit (an IR remote and a
UV/red/blue torch) lacks. **There is no evidence of a dedicated yellow channel** in the firmware
strings or on the wire; on an RGB screen "yellow" is red+green light, so any yellow response is just
the sum of the red and green channels.

## WHOOP 4.0 (gen4) also has a raw AFE stream

An earlier revision of this document said the raw stream was a gen5 feature. **That was wrong.**
WHOOP 4.0 exposes the same type-43 stream, started by the **same opcode `63` with the same `[0x01]`
revision byte**, on the gen4 service `61080001-8d6d-82b8-614a-1c8cb0f8dcc6` (4-byte envelope,
CRC-8 poly `0x07`). `[HW]` — verified on a worn WHOOP 4.0.

The subtypes are *not* byte-identical to gen5:

| | WHOOP 4.0 (gen4) | 5.0 / MG (gen5) |
|---|---|---|
| v0a channels | three 100-sample `u16` at `0x055` / `0x11d` / `0x1e5` — **same offsets** | same |
| v0a middle channel | **optical** (gen4 has no ECG electrode) | **ECG** |
| v0b channels | **two** 50-sample `i32`: red `0x026`, IR `0x0ee`, contiguous | three 25-sample `i32` |
| v0b rate | **50 Hz** | 25 Hz |
| ambient reference | **absent** (`0x6b9` reads 0) | present at `0x6b9` |

**Channel identity, same lighting controls as gen5.** Both v0b channels rail *negative* in open air
(red → ~0, IR → ~−147 000) and are positive on skin, so both are reflective LED PPG; both clip at
**524 287 = 2¹⁹−1** under a saturating white light, confirming a ~20-bit signed ADC. An infrared
source inverts their ratio to IR/red ≈ 2.2 against a worn baseline of 0.69, and a red source drives
the red channel up while IR sits negative — so **`0x026` is red and `0x0ee` is IR**, the same
ordering as gen5. The IR discrimination rests on a single strong frame, so treat the red/IR
assignment as well-evidenced rather than exhaustively proven.

**Practical gotchas.** The official WHOOP app holds the strap's single connection slot — a competing
`connectGatt` returns `status=133` until that app is stopped. And a phone may carry several WHOOP
bonds (a renamed strap keeps its old bond record too), so target the strap by its exact bonded name
and log the discovered service before trusting a capture.

## The firmware config table (14 keys)

The config key exchange (`117 [0x01]` opens, `118 [0x01]` walks) enumerates exactly **14** keys on
this firmware — the first time this table has been listed:

```
general_ab_test          enable_r22_v5_packets    enable_pdaf_walk_det
enable_r22_packets       enable_r22_v6_packets    enable_maverick_model
enable_r22_v2_packets    enable_r22_v8_packets    hr_ch_switching
enable_r22_v3_packets    disable_pip_r26_packets  ir_hw_switching
enable_r22_v4_packets    wear_detect_bias
```

`SET_CONFIG` (opcode 120) accepts **only** these keys (status 1); any other name — `make_hrfm_visible`,
`enable_passive_strap_fit_gen5`, `enable_sig11_during_sleep`, `dorset_inhibit_wpt`, `enable_sig12`,
and crucially **`enable_raw_data_w_ecg`** — is rejected (status 0). The console confirms the walk with
`Persistent config index 14 is invalid`. Novel keys worth noting: `enable_maverick_model`,
`enable_pdaf_walk_det`, `general_ab_test`.

## The AFE and what else it exposes

Firmware console strings (`opt_ir`, `opt_amb`, `red`, `green`, `SIGPROC-WEAR-DETECT`) show a
multi-LED optical front end — **red, IR, green + ambient** — plus the single-lead ECG electrode.
Metrics WHOOP already derives from this: HR, HRV, SpO2, respiratory rate, skin temperature, ECG. The
**raw optical waveforms themselves are the latent, never-exposed data** — capturing them enables
independent SpO2 (red/IR ratio), respiratory rate (PPG/ECG modulation), motion-robust HR (green), and
PPG-morphology cardiovascular signals. No evidence of bioimpedance/BioZ on this AFE.

## Blood pressure

**Not computed on-strap.** No `pressure`/`systolic`/`bp` string appears in any config key, console
line, packet, or in the oracle. WHOOP's Blood Pressure Insights is **cloud-derived** from the raw
ECG + PPG (pulse-transit timing) with cuff calibration. The strap's job is to emit the raw waveforms —
which this stream now provides — so BP is a downstream signal-timing problem, not a wire problem.

## Sample-rate summary

| Data | Rate | Where |
|------|------|-------|
| ECG (electrode) | **100 Hz** | v0a channel @ `0x11d` |
| HR PPG (green, inferred) | **100 Hz** | v0a channels @ `0x055` / `0x1e5` |
| SpO2 red / IR / ambient | **25 Hz** | v0b `i32` channels @ `0x026` / `0x0ee` / `0x6b9` |
| live HR (already emitted) | ~1 Hz | packet 40 `REALTIME_DATA`, byte 8 = bpm |

## Where this lives in code

`maverick-connectors/crates/whoop-protocol/src/realtime_raw.rs` decodes the **v0a** frame
(`decode_realtime_raw` → `RawAfeFrame`) and defines `START_AFE_RAW` (63). The whoop5 connector emits
`ecg` / `ppg-raw-a` / `ppg-raw-b` samples from it, behind the `ecg-probe` feature so release builds
never stream raw (battery/bandwidth) and their signed artifacts stay byte-identical. The decoder is
general to gen5, so the PPG path is expected to work on a non-MG WHOOP 5.0 as well (untested — no
such unit available).

The **v0b** pulse-ox triad above is documented but **not yet decoded in code** — the connector still
routes only v0a. Adding it would be a small extension of `realtime_raw.rs` (three signed `i32`
channels at the offsets in the table), gated behind the same `ecg-probe` feature; it is left until
there is a consumer for raw SpO2 channels, per the standing rule of not surfacing raw streams that no
host stage reads yet. Hardware notes and captures for all of the above live in the standalone
`whoop-mg-android` lab tool, outside this repo.
