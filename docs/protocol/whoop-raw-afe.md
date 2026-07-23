# WHOOP 5.0 / MG raw AFE stream (ECG + PPG)

The raw analog-front-end (AFE) stream carries the WHOOP MG's single-lead **ECG** and the **optical
PPG** channels straight off the sensor, undecimated. No public source had decoded it; the docs, this
project's own connector, and the `whoop-rs` oracle all chased the wrong command. This document is the
result of cracking it on live hardware.

**Confidence:** `[HW]` — verified against a worn WHOOP MG, strap `MGB0261172`, firmware **50.33.2.0**,
over Bluetooth from a standalone Android tool bonded to the strap. Captures and the tool live outside
this repo (`whoop-mg-android/`); a memory note records the paths.

---

## TL;DR

- **Start the stream with opcode `63` and a `[0x01]` revision byte.** Not `START_RAW_DATA` (81).
- 81 is *accepted* (status OK) but streams nothing. `enable_raw_data_w_ecg` is **not a config key on
  this firmware** and is always rejected. Both were red herrings the whole ecosystem believed.
- The stream is packet type **43 `REALTIME_RAW_DATA`**, two interleaved subtypes, **one pair per
  second**.
- Subtype **v0x0a** (1920 B) carries **three 100-sample `u16` channels at 100 Hz**: PPG, **ECG**, PPG.
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
| PPG A | `0x055` | ~470 | optical |
| **ECG** | `0x11d` | ~1220 | **single-lead electrode** |
| PPG B | `0x1e5` | ~3900 | optical |

100 samples per one-second frame ⇒ **100 Hz** per channel. Sample *i* is at
`unix_time * 1000 + i * 10` ms. After the channels come two ~25-sample `u32` blocks (≈25 Hz derived
values); the rest of the frame is zero padding.

**How the ECG channel was identified — the electrode-contact control.** With a finger on the metal
electrode the middle channel (`0x11d`) is a stable ~1220 baseline carrying the biopotential; with the
electrode *floating* the same channel rails between the ADC extremes (0 ↔ ~4000) and its variance
explodes (σ ≈ 475 vs 99). The two optical channels barely move either way — they only need skin
proximity, not the electrode. That flip *is* the proof of which channel is the ECG lead.

### Subtype v0x0b (not decoded)

1924 B, `u32` optical channels (~449k, ~414k baselines) — full-resolution / DC AFE data. Left
undecoded on purpose: no derived-metric maths are done on-strap, so the raw channels are surfaced and
the interpretation is deferred to the host.

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
| PPG A / PPG B | **100 Hz** | v0a channels @ `0x055` / `0x1e5` |
| derived u32 blocks | ~25 Hz | v0a tail |
| full-res u32 optical | per v0b | v0b (undecoded) |
| live HR (already emitted) | ~1 Hz | packet 40 `REALTIME_DATA`, byte 8 = bpm |

## Where this lives in code

`maverick-connectors/crates/whoop-protocol/src/realtime_raw.rs` decodes the v0a frame
(`decode_realtime_raw` → `RawAfeFrame`) and defines `START_AFE_RAW` (63). The whoop5 connector emits
`ecg` / `ppg-raw-a` / `ppg-raw-b` samples from it, behind the `ecg-probe` feature so release builds
never stream raw (battery/bandwidth) and their signed artifacts stay byte-identical. The decoder is
general to gen5, so the PPG path is expected to work on a non-MG WHOOP 5.0 as well (untested — no
such unit available).
