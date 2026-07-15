# WHOOP protocol reference

Read this before you write anything that touches a WHOOP strap.

Every fact here was reverse-engineered from two prior codebases and from packet captures. Not one
line has been confirmed against a physical strap, because at the time of writing we have none; a
WHOOP 4.0 and a 5.0/MG are on order. So each fact carries a confidence tag, and the tag is the most
important thing on the line. When the straps arrive, verifying this document is a checklist, not an
excavation: every tag that is not yet hardware-verified is an item to confirm or correct against a
live capture. Do not treat a high-confidence tag as a settled fact; treat it as a strong prior that
still owes a hardware check.

The tags:

- **[XVAL]** — both surveyed codebases agree. High confidence, still verify on hardware eventually.
- **[ONE]** — only one codebase asserts it. Medium confidence; the source is named where it matters.
- **[PROV]** — provisional, uncalibrated, or self-admittedly guessed. Treat as an approximation.
- **[HW]** — can only be confirmed with a physical strap, which we do not have.
- **[CONFLICT]** — the two codebases disagree. Must be resolved on hardware; do not hardcode a guess.

The two sources are referred to below as the two surveyed codebases, one a Rust core with real
capture fixtures, the other a Swift and Kotlin implementation, also with real fixtures and honest
provenance comments. A standing rule for this document, learned from both of them: a protocol claim
must cite a code location or a fixture, because prose docs drifted from code in both repos (one had
a wrong gen5 frame layout and a UUID typo, the other referenced a manifest file that did not exist).
`tools/check_docs.sh` fails on dead cross-references for the same reason.

## Device families

WHOOP 4.0 is "gen4". WHOOP 5.0 and WHOOP MG are "gen5" and share one wire family. [XVAL]

The 5.0 and the MG cannot be told apart at BLE scan time, because they advertise the same service
UUID; the registry or model string is what disambiguates them. [XVAL] An unknown or legacy model
string defaults to gen5. [ONE, from the Swift/Kotlin repo] MG capabilities are assumed identical to
the 5.0 based on static analysis of the Android app, and were never confirmed on hardware. [HW]

## GATT

| role | gen4 (4.0) | gen5 (5.0 / MG) |
|---|---|---|
| service | `61080001-8d6d-82b8-614a-1c8cb0f8dcc6` | `fd4b0001-cce1-4033-93ce-002d5875f58a` |
| command (write) characteristic | `61080002-…` | `fd4b0002-…` |
| notify characteristics | `61080003 / 0004 / 0005 / 0007` | `fd4b0003 / 0004 / 0005 / 0007` |

All of the above is [XVAL]. Of the notify characteristics, `…0004` carries events and wrist on/off,
and `…0007` is a debug menu. [ONE, Rust repo]

The straps also expose standard GATT characteristics: Heart Rate `180D / 2A37` (which works
**unbonded** on the 4.0), Battery `180F / 2A19`, Battery Level Status `2BED`, and Device Information
`180A`. [XVAL] Note the Battery Level Status UUID is `2BED`; one source's prose said `2BEB`, which
is a documentation typo, and its code has `2BED`. Trust the code.

## Frame format

Trust the code here, not prose: both surveyed repos had frame documentation that was wrong in
places, so the following is taken from their decoders and their fixtures, not their comments.

The start-of-frame byte is `0xAA`. Three CRCs are used across the two generations, all [XVAL]:

- CRC-8, polynomial `0x07`, init `0x00`.
- CRC-16/Modbus, polynomial `0xA001`, init `0xFFFF`.
- CRC-32, zlib, reflected `0xEDB88320`.

### gen4, 4-byte header

```
[0]      0xAA
[1..2]   declared_len  u16 LE   (= payload_len + 4, includes the trailing CRC32)
[3]      CRC8(bytes[1..=2])
[4..]    payload (NOT padded)
[tail]   CRC32(payload)  u32 LE
total  = declared_len + 4
```

### gen5, 8-byte header

```
[0]      0xAA
[1]      0x01              format / version
[2..3]   declared_len  u16 LE   (= padded_payload_len + 4)
[4..5]   0x00 0x01         reserved
[6..7]   CRC16/Modbus(bytes[0..=5])  LE
[8..]    payload, zero-padded to a 4-byte boundary
[tail]   CRC32(padded payload)  u32 LE
total  = declared_len + 8
```

Both layouts are [XVAL]. The proof both codebases cite is a static gen5 hello frame,
`aa0108000001e67123019101363e5c8d`, which decodes exactly under the 8-byte layout.

### Reassembly

Buffer incoming bytes, discard everything before an `0xAA` start-of-frame, peek the declared length
to work out the expected frame length (gen4 `len + 4`, gen5 `len + 8`), emit the frame once at least
that many bytes are buffered, and resynchronise on garbage. Guard the whole thing with a maximum
frame size of about 8192 bytes so a runaway length cannot allocate without bound. [XVAL] A truncated
frame is tolerated only for realtime and historical data packet types, and never for a command.
[ONE, Rust repo]

## Inner payload

Inside a validated frame, the body has a fixed small header: [XVAL]

```
[0]  packet_type
[1]  sequence
[2]  command / event / subtype  (call it k)
[3..] body
```

The packet types, all [XVAL]:

| value | meaning |
|---|---|
| 35 | COMMAND |
| 36 | COMMAND_RESPONSE |
| 40 | REALTIME_DATA |
| 43 | REALTIME_RAW_DATA |
| 47 | HISTORICAL_DATA |
| 48 | EVENT |
| 49 | METADATA |
| 50 | CONSOLE_LOGS |
| 51 / 52 | IMU streams |
| 16 (0x10) | R22_REALTIME_DATA (the gen5 realtime path) |

An unknown packet-type byte becomes `Unknown(u8)` and is logged; it never panics. [XVAL]

## Commands (opcodes)

The subset both codebases agree on is high confidence: [XVAL]

| opcode | command |
|---|---|
| 3 | toggle_realtime_hr |
| 10 | set_clock |
| 11 | get_clock |
| 22 | send_historical_data |
| 23 | historical_data_result (trim ack) |
| 26 | get_battery_level |
| 34 | get_data_range |
| 63 | send_r10_r11_realtime (type-43 raw stream on/off) |
| 145 | get_hello (gen5 handshake) |

The gen5 deep-data unlock (the R22 path) is a feature-flag channel rather than a single opcode.
`set_feature_flag_value` is 120 (`0x78`). [XVAL] The Swift/Kotlin repo also has 119 (`0x77`),
`set_device_config`, used to broadcast HR in the advertising indication; the Rust repo describes the
fuller key-exchange chain of 117 (start), 118 (send next), and 120 (set). R22 is the gen5 realtime
capability (packet `0x10`), gated by capability rather than by any one opcode. [ONE each, and the
two accounts are consistent with one another]

There is one outright disagreement. Entering and exiting high-frequency historical sync is opcodes
85 / 86 in the Rust repo and 96 / 97 in the Swift/Kotlin repo. [CONFLICT] This must be resolved on
hardware; do not hardcode either guess without a fixture that proves it.

### No transport encryption

The handshake carries no cryptography. On gen5 it is a write of the fixed 16-byte hello, which is
`GET_HELLO` (command 145, sequence 1, data `[0x01]`); on gen4 it is the same `GET_HELLO` in gen4
framing. Frames are CRC-checked and never encrypted. [XVAL]

The real security is OS-level BLE bonding (SMP pairing), not anything in the payload. A gen5 command
characteristic write fails until the phone is bonded to the strap, whereas the gen4 heart-rate
characteristic `2A37` works unbonded. One practical consequence is that macOS CoreBluetooth cannot
complete the gen5 bond, so config and R22 writes are only possible from iOS and Android. [XVAL] This
is why the decrypt hook in the acquisition stage is a pass-through for WHOOP: there is nothing to
decrypt, and access control lives in the radio, not the bytes.

## Realtime and event decode

REALTIME_DATA (packet 40): timestamp `u32` at offset 6, sub-second `u16` at 10, HR `u8` at 12,
`rr_count` `u8` at 13, and then `rr[i]` as `u16` LE at `14 + 2i` in milliseconds, dropping any
zero-millisecond placeholder. On gen5 the same fields sit at the same offsets plus 4. The HR value
was cross-checked against the standard `2A37` characteristic to within half a beat per minute.
[XVAL]

R22_REALTIME (packet `0x10`): battery percentage is a direct `u8` in `0..=100`, and HR is a `u16`
of milli-bpm divided by 10. [ONE, Rust repo]

EVENT (packet 48): event byte `u8` at offset 6, timestamp `u32` at 8 (a real RTC value). The battery
event on gen4 carries state of charge as `u16` at offset 17 in deci-percent (divide by 10),
millivolts at 21, and a charging flag in bit 0 at offset 26; on gen5 add 4 to each offset. [XVAL]

The standard `2A37` RR conversion is `raw * 1000 / 1024` milliseconds. [XVAL]

`COMMAND_RESPONSE` for get_battery differs by generation: gen4 returns `u16 / 10`, gen5 returns a
direct percent byte. This is a genuine 4.0-versus-5.0 difference, not a decode ambiguity. [ONE,
Swift/Kotlin repo]

## Historical records

Historical records are versioned by the sequence or subtype byte, and each version has its own field
layout. The absolute byte offsets in the surveys sometimes differ because one counts from the frame
start and the other from the body start; where that happens the field order and types still match,
and the exact offsets should be pinned from a fixture rather than copied from prose.

### gen4 V24 — the primary biometric history record

Both repos agree on the field set of V24. [XVAL fields] The record carries a unix timestamp `u32`,
HR `u8`, `rr_count` and an array of `rr[]` `u16` LE, `ppg_green` `u16`, `ppg_red_ir` `u16`, a
gravity triplet of three `f32` in g-units, a second gravity triplet of three `f32`, `skin_contact`
`u8`, `spo2_red` `u16`, `spo2_ir` `u16`, `skin_temp_raw` `u16`, `ambient` `u16`, two LED-drive
values, `resp_raw` `u16`, and `signal_quality` `u16`.

Not all of those are equally trustworthy. The decode maturity for V24, [XVAL]:

- **Solid:** unix timestamp, HR, `skin_contact`, the second gravity triplet, SpO2 red and IR raw,
  `skin_temp_raw`, `resp_raw`, `signal_quality`.
- **Candidate** (parsed but not trusted): `rr[]`, `ppg_green`, `ppg_red_ir`, the first gravity
  triplet. [PROV]
- **Not decoded:** the PPG flags, several byte ranges, `ambient`, and the LED-drive values.

### Other record versions

Each of these is asserted by one codebase, at medium confidence. [ONE each]

- **gen4 v25** (Swift/Kotlin repo, 84 bytes): motion, timestamp, and a PPG waveform, with no
  per-second HR. It feeds the sleep stager and is magnitude-gated to 0.5–1.5 g.
- **gen4 v5 / v7 / v9:** generic HR/RR-only records.
- **gen5 v18:** a per-second record carrying `record_index`, unix timestamp, HR `u8`, `rr[]`, an
  8.8 fixed-point HR (`bpm = val / 256`, correlation 0.989 against the `u8` HR), a gravity `f32`, a
  step counter, `skin_temp_raw` in centidegrees, and a sleep-state nibble. The Rust repo notes that
  the HR offset was corrected after cross-validation against a Garmin, because an earlier offset was
  reading a dead zero region; the offsets here were partly guessed. [PROV on the exact offsets]
- **gen5 v20 / v21:** bulk multi-channel optical data (100-sample `i16` groups, or presence-gated
  50-sample `i32` blocks). Raw channels, with no asserted LED mapping.
- **gen5 v26:** a 24 Hz optical PPG waveform, 24 `i16` LE per record, one record per second. Raw
  ADC, with no invented scale.
- **Unmapped versions:** fall back to the V24 layout, then **reject** if the gravity magnitude or HR
  is outside a physiological range. The device data is the arbiter; do not store garbage. [ONE,
  Swift/Kotlin repo]

## Unit conversions

- **HR:** whole bpm as a `u8`, except R22 (milli-bpm / 10) and gen5's 8.8 fixed-point (`/ 256`).
  Plausibility roughly `[30, 220]` bpm.
- **RR:** whole milliseconds from gen4 and history; `raw * 1000 / 1024` ms from the GATT
  characteristic. Valid roughly `[300, 2000]` ms, then a Malik / Lipponen–Tarvainen ±20% ectopic
  filter before any HRV is computed. [XVAL]
- **SpO2:** stored as raw ADC on the device. If it is computed, the ratio-of-ratios is
  `R = (AC_r / DC_r) / (AC_ir / DC_ir)` and `SpO2 = clamp(110 - 25R, 70, 100)`. Those constants are
  from a TI textbook and are uncalibrated. [PROV]
- **Resp:** a Welch PSD peak on the roughly 1 Hz `resp_raw` in the 0.1–0.5 Hz band gives breaths per
  minute. The scale of `resp_raw` is literally unknown, so it is left at 1.0. [PROV]
- **Battery SoC:** deci-percent from events; direct percent from the gen5 get_battery response and
  from R22.
- **IMU:** accelerometer at `1/4096` g per LSB; gyroscope at `2000/32768 = 0.06104` deg/s per LSB
  (±2000 dps). Verified on gen4 by a sphere fit and a 720° rotation. [XVAL]

### Skin temperature

Skin temperature deserves its own heading because it is the conversion most likely to be wrong and
the two codebases disagree about it.

On gen5, `skin_temp_raw` is centidegrees, so `°C = raw / 100`. Both repos agree. [XVAL]

On gen4 there is no agreement on the absolute scale.

- The Rust repo uses a fixed anchor: `delta_c = (raw - 930) / 30`, with raw 930 read as zero delta
  at 33 °C. [ONE]
- The Swift/Kotlin repo uses a learned per-device affine fit: `°C = 33.0 + (raw - anchorRaw) * 0.05`,
  where `anchorRaw` defaults to 826 but is learned from the worn-band median for that specific
  device. [ONE]

Both repos say explicitly that the gen4 absolute skin temperature is provisional, and that only the
deviation from the device's own baseline is defensible. [PROV / CONFLICT]

The design consequence for Maverick is set out in [../connectors.md](../connectors.md), and it is
worth restating here because it drives a piece of the architecture. Skin temperature is modelled as
a per-device learned anchor plus slope, which makes the fixed-anchor approach a special case of the
learned one, stored in the per-device key-value table, and surfaced as a deviation from personal
baseline rather than as an absolute thermometer reading, until there is a hardware calibration. This
is exactly the kind of value a manifest cannot hold and a `DeviceCodec` must, which is why manifests
in Maverick are not purely declarative.

## Raw sensor streaming

- **Type-43 raw realtime (R10/R11):** controlled by command 63. Send `[0x00]` on connect to stop the
  flood, or the strap will keep streaming raw samples.
- **gen5 raw 6-axis IMU offload:** roughly a 1244-byte buffer of 100 accelerometer `i16` and 100
  gyroscope `i16`, columnar, at 100 Hz. A live IMU request (command 106) is refused by the firmware;
  the offload path is what actually yields data. [ONE, Swift/Kotlin repo]
- **PPG:** gen5 v26 gives 24 Hz; gen4 raw optical is roughly 437 Hz on a single green channel as
  `s24` LE. [ONE each]

## Historical sync and backfill

The wire sequence is [XVAL]:

**gen5:** `GET_DATA_RANGE` (34) then `SEND_HISTORICAL_DATA` (22), whose result 2 means pending and
may repeat while 1 means ok. The strap then streams records with `HISTORY_START` and `HISTORY_END`
metadata, the app acknowledges each burst with `HISTORICAL_DATA_RESULT` (23), and the exchange ends
with `HISTORY_COMPLETE`.

**gen4:** `GET_DATA_RANGE` (34, `[0x00]`) whose response carries the last-synced page sequence as a
little-endian `u32`, so the next page is `last + 1`. Then `SEND_HISTORICAL_DATA` (22, `[0x00]`), a
short `0x02` ack, `HISTORICAL_DATA_RESULT` (23, `[0x01, LE32 seq, page_count]`), the record stream
and a `HISTORY_END`, then increment the sequence and repeat until `HISTORY_COMPLETE`.

Metadata kinds are 1 START, 2 END (which carries the ack payload and trim cursor), and 3 COMPLETE.

### The safe-trim invariant

The Swift/Kotlin repo's backfiller is careful in a way worth copying exactly. A chunk of history is
forgotten from the strap only after it has been durably stored on the phone **and** the link has
confirmed the trim. The order is: decode, persist durably, optionally enqueue the raw batch, advance
the cursor, then acknowledge the trim. A persist failure blocks all further acknowledgements, so the
cursor can never advance past history that was not stored, and a strap is never told to discard data
the phone has not safely written. [ONE, Swift/Kotlin repo, and it is a strong point]

### Clock correction and plausibility

The approach is agreed even where the exact windows differ. [XVAL approach]

A device timestamp is used only if it falls in a plausible unix window. One repo uses 2000-01-01 to
2100-01-01; the other uses roughly `>= 1.7e9` with a one-day future margin, plus a session-relative
window from `GET_DATA_RANGE` of about ±7 days. Outside that window, the sample falls back to the BLE
capture time and is flagged.

The Swift/Kotlin repo also snaps a grossly stale RTC offset to a 5-minute grid, so that a later
re-sync deduplicates cleanly, with a guard against overshooting past the current time. [ONE]

There is one unresolved inconsistency to be careful about. `GET_CLOCK` sub-seconds are in units of
`1/32768`, but the sub-seconds inside a data packet are treated as milliseconds. The Rust repo flags
this as an inconsistency it never resolved. [ONE] Maverick must pick one interpretation per field
and document which, rather than carrying the ambiguity into the timeline.

### RR dedup keying

Key RR intervals on `(device, ts, rr_ms, seq)`, where `seq` is an in-second occurrence counter.
Without the `seq` tiebreaker, two equal RR intervals in the same second collapse into one, which
removes a real (zero-difference) beat and biases RMSSD high. [XVAL] This is the fix that landed as
noop PR #163, and its iOS parity commit is in the current NOOP git history. In Maverick it is an
invariant test in `mav-timeline`, described in [../pipeline.md](../pipeline.md).

## The hardware checklist

When the straps arrive, this document becomes a checklist. Every [CONFLICT] is resolved against a
live capture (the high-frequency-sync opcodes first). Every [PROV] value is calibrated or confirmed
(the gen4 skin-temp scale, the SpO2 constants, the resp scale, the guessed gen5 v18 offsets). Every
[HW] assumption is checked (MG capability parity with the 5.0). Every [XVAL] fact is spot-checked,
because agreement between two reverse-engineered codebases is a strong prior, not a measurement.
Fixtures are regenerated from real captures, and the tags in this file flip to hardware-verified as
each item is confirmed.
