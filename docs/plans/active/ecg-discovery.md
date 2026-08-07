# ECG-discovery lane — finding the MG's waveform on the wire

> **RESOLVED (2026-07-23). The ECG is decoded.** The raw AFE stream is started by **opcode `63` with a
> `[0x01]` revision byte** — not `START_RAW_DATA` (81), which is accepted but silent, and not
> `enable_raw_data_w_ecg`, which is not a config key on this firmware. It produces packet type 43 at
> **100 Hz**: three `u16` channels (two optical PPG, and the single-lead ECG between them, pinned by an
> electrode-contact control capture). No subscription was needed — it was the wrong opcode all along.
> The full, clean reference is **[`docs/protocol/whoop-raw-afe.md`](../../protocol/whoop-raw-afe.md)**;
> the decoder shipped in `whoop-protocol::realtime_raw`. The narrative below is kept as the record of
> how it was cracked (and of the two red herrings that cost the most time).

The WHOOP MG carries a single-lead ECG. When this lane opened, **no source had decoded it** — it was
in no mapped record, no source had found the command or record type that carries it, and we had no
official app or subscription to observe it properly. This lane is genuine reverse-engineering on our
own hardware, written down as it goes because the failures are as informative as the wins.

Everything here runs against the user's own strap under the repository's standing refusal set: the
destructive opcodes (trim, DFU) are never sent, and the only persistent write is a config flag that
the firmware itself exposes and that is reversible.

---

## What the sources establish

- The MG's ECG is **electrode-gated and MG-only**. The 5.0 firmware refuses it outright
  (`ECG Control not supported on Goose hardware`; Goose is the PPG-only 5.0). The MG gates it behind
  the electrodes *and* a config key. [FIELD]
- The firmware carries HeartKey strings (`HK … Afib %d … #ECG %d`), so the path exists in software.
  [FIELD]
- One config flag names it: **`enable_raw_data_w_ecg`**. It is present in firmware and deliberately
  absent from the R22 sequence — the sixteen flags the app sends on every connect. [WRS]
- The stream that flag qualifies is opened by **`START_RAW_DATA` (81)**, closed by `STOP_RAW_DATA`
  (82). Neither is destructive or persistent. [WRS]
- That stream's packet type is **43, `REALTIME_RAW_DATA`**. Every source names it. **None decodes
  it.** [WRS]

That chain — a flag named for ECG, gating a stream, whose packet type nobody has decoded — is the
hypothesis this lane tests.

## What this repository was missing before the lane started

Three things, each of which would have silently swallowed the answer:

1. **The packet vocabulary stopped at six types.** 43 and the whole Puffin command channel
   (37/38/53/54/56) plus the IMU streams (51/52) were not even named, so a type-43 frame arrived as
   an anonymous decode failure. `PacketKind` now names all fifteen.
2. **The host discarded every connector diagnostic message.** `EmitDiagnostic` was journalled as
   `"connector emitted a diagnostic"` with only its code — so a connector could report the bytes of
   an undecoded frame and the host would throw the bytes away. Fixed: the message and level are
   carried through (codes 11063–11065).
3. **Duplicate-sample notices flooded the journal.** A real capture wrote 495 of them in a 500-row
   window, burying everything else. Now journalled on a doubling ladder.

## The probe

`connectors/whoop5` gains a `ecg-probe` Cargo feature — a feature, not a runtime toggle, so a probe
build is a different artifact and cannot ship by accident. It:

- appends `SET_CONFIG enable_raw_data_w_ecg = '2'` after the R22 sequence;
- sends `START_RAW_DATA`;
- kicks a history offload immediately after, because the oracle's own raw-stream recipe notes that
  type-43 "rides that window";
- reports **every** frame whose packet type has no decoder as bounded hex;
- reports **every** command response with no reviewed decoder, with its status — which is how we
  learn whether the strap accepted, refused, or ignored opcode 81.

Built, signed with a throwaway development-scope key, and sideloaded onto the device. Version 1.900.x
is reserved for discovery builds and never released.

## Runs so far

**Run 1 (v1.900.0), strap on a desk, off wrist.** Connected, configured, streamed. The full R22
sequence, the ECG flag, and `START_RAW_DATA` all went out — confirmed by the end-of-config diagnostic
firing after them. Result: **no type-43 frames, no unmapped packets at all.**

Two facts came out of it anyway, both hardware-verified firsts for this project:

- `strap MGB0261172, firmware [50, 33, 2, 0]` — the gen5 hello decoder ported in WF-P10 works on
  real silicon, and the MG's firmware is **50.33.2.0**. (`docs/protocol/whoop.md` records a
  different unit at 50.38.1.0 from another source; both are 50.x.)
- `connector session operation budget exhausted` — a real session hits `MAX_SESSION_OPERATIONS`
  (4,096). Worth its own packet: a long raw-stream session will hit it far sooner.

**Run 2 (v1.900.1)** added the response and unmapped-packet reporting above. Same result: no type-43,
and the probe-response diagnostic never fired either, which suggests the strap sent no response to
opcode 81 at all rather than a refusal.

## The most likely reason, and the next run

**The strap was not being worn.** The telemetry line said so — `off wrist` — and ECG is
electrode-gated in firmware. A single-lead ECG needs a closed circuit: the strap on one wrist and a
finger from the other hand on the electrode. A strap sitting on a desk cannot produce a waveform no
matter what the wire is told, and it may well refuse to open the raw stream at all.

So the next run is a physical one, and it is the user's to perform:

1. Wear the strap, snug, on the wrist.
2. Connect with the probe build (v1.900.x, installed alongside the release connector).
3. Hold a finger of the opposite hand on the metal electrode for 30 seconds or so — the gesture the
   official app asks for during an ECG reading.
4. Disconnect, then export diagnostics from the Devices screen.

Anything on the wire will be in the journal as `whoop5-unmapped-packet` or `whoop5-probe-response`,
with hex.

## If that produces nothing

In rough order of expected value:

- **Read the console channel during the attempt.** The firmware narrates over `fd4b0007`, and the
  console decoder landed in WF-P9. If ECG control is refused, the firmware is likely to say so in
  as many words. This is the cheapest remaining lead and it needs no new opcodes.
- **Try the Puffin command channel.** Packet types 37/38 are a second command/response channel that
  no source decodes. An MG-only feature living on an MG-family channel is not a wild guess.
- **Sweep the read-only opcode space.** `build_command` refuses the destructive and persistent sets;
  everything else is a getter. A sweep with response logging is safe by construction and would map
  which opcodes the MG answers at all.
- **`SET_IMU_DATA_STREAM` (106) as a shape reference.** The oracle notes a live IMU request is
  refused by firmware; comparing its refusal to opcode 81's silence tells us whether silence means
  "not supported" or "not now".

## Explicitly not tried, and why

Firmware load, `FORCE_TRIM`, and the DFU entries are refused by `build_command` and always will be.
Nothing about ECG discovery requires writing firmware, and a bricked strap ends the investigation
permanently.

## Run 3 — the firmware told us what is wrong

Adding a diagnostic to every probe write and every command response turned the strap into a talking
witness. Three things came back, and together they change the picture completely.

**1. The console channel is fully readable, and the firmware narrates everything.** Sixty-five
CONSOLE_LOGS frames in one short session: BLE connect and disconnect with reasons, every
subscription index, the negotiated MTU, each command as it is received, sensor state changes,
battery SOC to two decimals, IMU double-tap, even the LED current array. This is the single most
valuable instrument we have, and it cost one diagnostic.

Note the routing: those frames arrived as **unmapped packets on a data channel**, not on `fd4b0007`.
Console output is not confined to the console characteristic.

**2. The config chain is half-broken, and the firmware says so in words.**

```
BLE_CMD: WSBLE_CMD_START_DEVICE_CONFIG_KEY_EX unsupported revision:0
BLE_CMD: WSBLE_CMD_SEND_NEXT_DEVICE_CONFIG   unsupported revision:0
BLE_CMD: WSBLE_CMD_CONFIG_VALUE_SET_DEVICE_CONFIG unsupported revision:101
```

Opcodes 117 and 118 are the "start" and "send next" halves of the config key exchange, and we send
both with **empty bodies**. The strap reads a revision byte, finds zero, and refuses. The key
exchange therefore never opens, and the SET_CONFIG writes that follow are parsed by a fallback path
that reports nonsense revisions (101, 109, 104 — which are the ASCII codes of `e`, `m`, `h`, the
first letters of the flag names, so the strap is reading a revision where we put a name).

**3. Which flags actually took, from the status byte the WF-P1 fix now reads correctly:**

| flag | result |
|---|---|
| `enable_r22_packets`, `_v2`, `_v3`, `_v4`, `_v5`, `_v6`, `_v8` | **Ok** |
| `disable_pip_r26_packets`, `wear_detect_bias`, `hr_ch_switching`, `ir_hw_switching` | **Ok** |
| `make_hrfm_visible` | rejected (status 0) |
| `enable_passive_strap_fit_gen5`, `enable_sig11_during_sleep` | rejected |
| `dorset_inhibit_wpt`, `enable_sig12` | rejected |
| **`enable_raw_data_w_ecg`** | **rejected (status 0)** |

So the ECG flag never took. That is why `START_RAW_DATA` produced nothing, and it is a far more
tractable problem than "the waveform is hidden somewhere unknown".

This also hardware-confirms WF-P1 end to end: the status byte at inner `[4]` cleanly separates the
accepted flags from the rejected ones. Read at the old offset (inner `[3]`) it is a counter —
`0x05, 0x06 … 0x15` — and every one of these results would have been noise.

## The next run

1. **Send opcodes 117 and 118 with a revision byte.** The firmware complains about `revision:0`,
   which is what an empty body reads as. Try `&[1]` first, then sweep 1..=8 while watching the
   console — the strap names the revision it rejected, so the sweep is self-reporting and cheap.
2. **Re-check the flag results once the key exchange opens.** If the eleven that already succeed are
   the pre-key-exchange subset, the rejected six — including the ECG flag — may simply require it.
3. **Only then re-try `START_RAW_DATA`.** Chasing packet 43 before the flag takes is chasing a
   stream that was never enabled.

Keep the console diagnostic on throughout. It has already earned its place twice.

## Run 4 — the config key exchange is open

> Point-in-time record on firmware **50.33.2.0**. The key count and table are versioned firmware
> state, not fixed — 50.41.1.0 announces **20** keys, `enable_raw_data_w_ecg` is in **no** version,
> and opcode **63** (not `START_RAW_DATA`) is the raw trigger. Current per-firmware tables live in
> [`whoop-raw-afe.md`](../../protocol/whoop-raw-afe.md#the-firmware-config-table); the hypotheses
> below about the ECG key were superseded by that work. [2026-08-07]

Swept revision bytes 1..8 across opcodes 117 and 118 in one session. The answer is **revision
`0x01`**, and it is unambiguous:

```
sending opcode 117 revision 1  →  Ok   24 be 75 15 01 01 0e 00
sending opcode 118 revision 1  →  Ok   24 bf 76 16 01 01 00 01 "general_ab_test"
sending opcode 117 revision 2  →  Unknown(0)
… 3, 4, 5, 6, 7, 8            →  Unknown(0)
```

Two facts fall straight out:

- **`117 [0x01]` opens the exchange** and answers with `0x0e` = **14** in its body — the number of
  config keys the firmware holds.
- **`118 [0x01]` walks the table**, answering with one key's **name** per call. The first is
  `general_ab_test` — which is in the oracle's `FIRMWARE_ONLY_FLAGS`, the set deliberately absent
  from the R22 sequence. So 118 enumerates the firmware's real config table, and
  `enable_raw_data_w_ecg` is in that table.

This explains every earlier failure. With an empty body the strap reads revision 0, refuses both
(`unsupported revision:0` on the console), the exchange never opens, and SET_CONFIG for any key the
firmware has not announced through 118 is rejected — which is exactly the five R22 flags that
failed, `enable_raw_data_w_ecg` among them.

The sweep also spoiled its own success: having opened the exchange with revision 1 it immediately
sent revisions 2..8, which the strap refused, closing it again. The corrected sequence is in
v1.902.0: `117 [0x01]` once, then `118 [0x01]` repeatedly to walk all fourteen keys, **then** the
ECG flag, **then** `START_RAW_DATA`.

## The next run, and it is a short one

v1.902.0 is built, signed, and on the phone at `/sdcard/Download/whoop5-ecg-probe.mavconn`. Install
it (Devices → Choose .mavconn document → approve), connect, and read the journal. What to look for:

1. **The fourteen key names**, from the `118 next key #N` responses. This is the firmware's own
   config table — the first time anyone has enumerated it — and it will confirm the exact spelling
   of the ECG key.
2. **Whether `enable_raw_data_w_ecg` now returns `Ok`** instead of `Unknown(0)`.
3. **Whether packet 43 appears** once the flag takes and `START_RAW_DATA` runs.

Wear the strap with a finger on the electrode for the last one; the wear marker now reports honestly,
so the diagnostics line will say `on wrist` when the circuit is right.

**Status: open, and one step from the answer.** The question is no longer "where is the ECG hidden"
or even "what revision" — it is "does the ECG key take once the exchange is open", and that is one
install away.
