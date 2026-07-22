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

- **[XVAL]** — sources agree. High confidence, still verify on hardware eventually.
- **[ONE]** — only one codebase asserts it. Medium confidence; the source is named where it matters.
- **[JUDES]** — from the third source's round-tripped MG capture. Strong for the gen5 wire, one unit.
- **[SERIES]** — from the fourth source's sniffed 4.0 and decoded MG buffer. Corpus-pinned, one setup.
- **[FIELD]** — from the fifth source, real worn-MG sessions. The closest thing to ground truth here,
  and it earns precedence over code-inferred claims where they collide; strongest on what it refutes.
- **[WRS]** — from the sixth source, `tanarchytan/whoop-rs`, a from-scratch Rust client that
  round-trips real 4.0 and 5.0/MG captures byte for byte; the origin of Maverick's real record
  fixtures. Strong for the wire, but its author's own hardware, so it still owes the check on ours.
- **[PROV]** — provisional, uncalibrated, or self-admittedly guessed. Treat as an approximation.
- **[HW]** — can only be confirmed with a physical strap, which we do not have.
- **[CONFLICT]** — the sources disagree. Must be resolved on hardware; do not hardcode a guess.

There are six sources. Two are surveyed codebases: one a Rust core with real capture fixtures, the
other a Swift and Kotlin implementation, also with real fixtures and honest provenance comments. The
third, tagged **[JUDES]** below, is a June 2026 writeup of cracking the 5.0 (judes.club), built on an
HCI capture of the official app talking to a real MG and validated by round-tripping all 8,031
captured frames byte for byte. The fourth, tagged **[SERIES]**, is a multi-part writeup that took a
4.0 apart with a hardware sniffer (nRF52840 plus BlueZ), then moved to the MG and decoded its
overnight buffer record by record, pinning every offset against a large stored corpus. The fifth,
tagged **[FIELD]**, is ongoing work on a real worn MG: it wears the band through full sessions and
correlates candidate fields against known state, so it is the only source that can tell a real signal
from a plausible-looking coincidence, and several of its results are refutations of the fourth
source's labels. Where [FIELD] and a code-inferred claim collide, [FIELD] wins.

The sixth, tagged **[WRS]**, is `tanarchytan/whoop-rs`: a from-scratch Rust WHOOP client (pure
sans-IO codec plus a BLE core) whose author decoded the 4.0 and 5.0/MG wire against his own real
captures, drained a real 5.0 overnight buffer record by record, and validated the derived metrics
against ground truth without leaning on guesses. It agrees with the surveyed repos and the three
writeups on the envelope, the command bytes, and the shared record offsets, and it is the origin of
the real 5.0/MG record frames now committed as Maverick's `fixtures/records/` goldens. It is the one
source that supplied Maverick with a real frame to decode rather than a synthetic one, so where its
capture settles a byte that the surveys left ambiguous, the capture wins.

The third, fourth, and fifth sources agree with the surveyed repos on the gen5 envelope and the
command bytes, which is about as much corroboration as reverse-engineering gets, but all of it is
still someone else's hardware and owes the same check on ours. A standing rule for this document,
learned from all six: a protocol claim must cite a code location or a fixture, because prose docs
drifted from code in the surveyed repos (one had a wrong gen5 frame layout and a UUID typo, the
other referenced a manifest file that did not exist), the fourth source is itself a catalogue of
plausible field labels that turned out wrong, and the fifth source's main product is disproving
some of the fourth's. `tools/check_docs.sh` fails on dead cross-references for the same reason.

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

On gen5 the notify characteristics carry, in order, command responses (`…0003`), events (`…0004`),
fragmented data (`…0005`), and a fourth channel new in the 5.0 (`…0007`). [JUDES]

The straps also expose standard GATT characteristics: Heart Rate `180D / 2A37`, Battery
`180F / 2A19`, Battery Level Status `2BED`, and Device Information `180A`. [XVAL] Note the Battery
Level Status UUID is `2BED`; one source's prose said `2BEB`, which is a documentation typo, and its
code has `2BED`. Trust the code.

The standard heart-rate profile is a real, shallow data source: a flags byte, then heart rate, then
RR intervals when the flags say they are present, giving live HR and RR with no custom-service
protocol at all. On the 4.0 it answers **unbonded**. On the 5.0 it needs the phone's OS bond like
everything else, because the whole device sits behind authenticated pairing. [JUDES, and the 4.0
half is XVAL] It carries no motion, no raw optical signal, and no stored history; those live behind
the bond and the enable handshake below.

Device Information reports a model string that disambiguates units the custom service cannot: a
captured MG reported model `MG`, hardware `WS50_r03`, firmware `50.38.1.0`. [JUDES] Since the 5.0
and the MG share a custom-service UUID and are identical at scan time, the model string read after
connect is the clean way to tell them apart.

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
[4..5]   src, dst          routing: 0x00 0x01 phone-to-strap, 0x01 0x00 the other way
[6..7]   CRC16/Modbus(bytes[0..=5])  LE
[8..]    payload, zero-padded to a 4-byte boundary
[tail]   CRC32(padded payload)  u32 LE
total  = declared_len + 8
```

Both layouts are [XVAL], and the gen5 envelope is now the best-attested fact in this document:
the third source round-tripped all 8,031 frames of its capture through exactly this layout, encode
equalling decode for every one. The two bytes at `[4..5]` are a source and destination pair rather
than a fixed constant, `0x00 0x01` on the way to the strap and `0x01 0x00` on the way back; because
the header CRC-16 covers them, a reassembler that validates the CRC over the received bytes handles
both directions without special-casing. The strap does not itself validate the header CRC-16 (the
third source found arbitrary values at `[6..7]` accepted for outbound commands), but we compute and
check it anyway, because it is the only integrity check on the header and it costs nothing. [JUDES,
and the src/dst routing is [ONE] from the fourth source]. The proof all sources cite is the static
gen5 hello frame `aa0108000001e67123019101363e5c8d`, which decodes exactly under the 8-byte layout
and which the `whoop-protocol` crate in `sennnen/maverick-connectors` reproduces byte for byte in
its `maverick_generation_goldens_decode_exactly` reference vectors. The core carries no copy: it
names no device wire format (ADR-012).

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

For a type-47 HISTORICAL_DATA record the layout version is byte `[1]` (the sequence position), not
byte `[2]`. A real worn 5.0/MG record carries the version there — 18 for the per-second metrics
record, 26 for the raw-PPG burst — and puts the on-wrist r22 command `0x80` in `[2]`. This resolves
the "sequence or subtype byte" ambiguity below in favour of the sequence byte; it is pinned by the
`[WRS]` real captures now in `fixtures/records/r20_k18_v1.json` and `r20_k26_v1.json`, and it is what
the WHOOP 5 `.mavconn` decoder keys on. An earlier synthetic Maverick fixture placed the version in
byte `[2]`; no real frame agrees with it, which is exactly the drift the "regenerate from a real
capture" rule exists to catch. [WRS]

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

The command and response types are `0x23` (35, COMMAND) and `0x24` (36, COMMAND_RESPONSE), confirmed
directly from sniffed wire bytes on both a 4.0 and an MG. [SERIES, and XVAL with the surveyed repos]
Older community documentation listed these as `0x72` and `0x73`; that was never an on-wire value,
just a red herring carried forward through several write-ups, and it produced complete silence for
anyone who trusted it. This is the clearest warning in the whole document about trusting a number
because it is written down: the only way anyone found the real bytes was to watch the official app
put them on the wire.

## Commands (opcodes)

The subset both codebases agree on is high confidence: [XVAL]

| opcode | command |
|---|---|
| 3 | toggle_realtime_hr |
| 10 | set_clock |
| 11 | get_clock |
| 22 | send_historical_data |
| 23 | historical_data_result (cursor acknowledgement; does not delete) |
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

There is one outright disagreement on high-frequency sync. Entering and exiting it is opcodes 85 / 86
in the Rust repo and 96 / 97 in the Swift/Kotlin repo. [CONFLICT] This must be resolved on hardware;
do not hardcode either guess without a fixture that proves it.

There is a second disagreement, on the hello opcode, and it splits by generation. The fourth source
sniffed a 4.0 sending its hello as command `0x23` (35), which it calls `GET_HELLO_HARVARD`, and an
MG sending `0x91` (145), which is the plain `GET_HELLO`; it is explicit that the newer straps moved
the hello to its own opcode. [SERIES] The surveyed repos used 145 for both generations. All sources
agree the gen5 hello is 145; the gen4 hello is the conflict. [CONFLICT on gen4] Because the fourth
source is a direct wire capture (`aa 10 00 57 23 04 23 …`, where the inner `23 04 23` is type 0x23,
sequence, command 0x23), the `whoop4` manifest uses 35 and records the conflict; a fixture from our
own 4.0 settles it.

More historical-sync commands are now confirmed from sniffed traffic. [SERIES] `0x16` (22) requests
history and `0x17` (23) acknowledges a burst and advances the strap's read pointer without deleting
anything. `HISTORY_COMPLETE` is a metadata packet (type 49) with command 3, emitted only by the
strap; a client cannot ask for it. And `0x19` (25), `FORCE_TRIM`, is the one that deletes the buffer
up to the write head, irreversibly. The safe-trim invariant below is not optional discipline: the
fourth source lost five hours of recording to a daemon that trimmed on an error path before
`HISTORY_COMPLETE` had arrived.

### Outbound alarm and haptic commands

The sibling `whoop-protocol` crate builds two families of outbound command frame, ported byte-for-byte from the
sixth source. Each is a pure function returning the complete `[COMMAND, seq, opcode] + body` frame
for one generation; nothing sends yet, because the runtime has no outbound-command send lane (the
handshake/historical machines produce commands, but the host runtime does not yet turn them into a
`Write`).

- **Haptic buzz.** gen5/MG is `RUN_HAPTIC_PATTERN_MAVERICK` (19) with the notification preset body
  `[01 2F 98 00…]` (the 47/152 waveform-effect pair); gen4 is the generic `RUN_HAPTICS_PATTERN`
  (79) with `[pattern_id, loops, 0, 0, 0]`, pattern 2 being the graduated alarm buzz. The
  Haptic-Clock schedule (long pulse = a ten, short = a unit) is a pure `Pulse` list a caller
  sequences. [WRS]
- **Wake alarm.** gen5/MG is `SET_ALARM_TIME` (66) REVISION_4: a 20-byte body carrying an epoch
  (seconds + `ms·32768/1000` subseconds), the alarm id, the waveform effects, and a 30 s / 7-loop
  envelope; `DISABLE_ALARM` (69) REVISION_2 is `[02 FF]`. gen4 is a 9-byte minute-precision body
  `[01][u32 epoch s][0000][0000]`. **EXPERIMENTAL/UNCONFIRMED [WRS]:** the upstream flags that these
  alarm bodies are not confirmed to actually wake a strap, so a surface must present the alarm as
  experimental, never as a guaranteed wake. Neither opcode is in the forbidden or destructive set.

### No transport encryption

The handshake carries no cryptography. On gen5 it is a write of the fixed 16-byte hello, which is
`GET_HELLO` (command 145, sequence 1, data `[0x01]`); on gen4 it is the same `GET_HELLO` in gen4
framing. Frames are CRC-checked and never encrypted. [XVAL]

The real security is OS-level BLE bonding (SMP pairing), not anything in the payload. A gen5 command
characteristic write fails until the phone is bonded to the strap, whereas the gen4 heart-rate
characteristic `2A37` works unbonded. One practical consequence is that macOS CoreBluetooth cannot
complete the gen5 bond, so config and R22 writes are only possible from iOS and Android. [XVAL]
WHOOP connectors therefore perform no payload decryption; access control lives in the radio bond,
not the bytes.

## Realtime and event decode

REALTIME_DATA (packet 40), inner-relative (the base every offset in this section now uses — the
inner record whose byte `[0]` is the packet type): timestamp `u32` at inner 2, sub-second `u16` at 6,
HR `u8` at 8, `rr_count`
`u8` at 9, and then `rr[i]` as `u16` LE at `10 + 2i` in milliseconds, dropping any zero-millisecond
placeholder. These positions are identical on both generations; the survey numbers (gen4 6/10/12,
"gen5 plus 4") counted from the full frame, whose header is 4 bytes on gen4 and 8 on gen5, and the
manifests carried those frame-relative numbers until the `[WRS]` real captures pinned the
inner-relative ones (its realtime decoder reads ts@2/HR@8/count@9/rr@10 against live frames, and
one decoder serves both generations). The HR value was cross-checked against the standard `2A37`
characteristic to within half a beat per minute. [XVAL fields, WRS byte positions]

The on-wrist live r22 biometric rides packet 47 (not 40), and it is not a separate record: the
sixth source's live decoder reads the same K=18 shape the historical path pins — unix at
`body[4]`, HR at `body[11]`, the secondary HR at `body[26]`, gravity at `body[34]` — just
minimally, for display. Its "fork on the command byte" (`0x80`/`0x82`, the on-wrist marker) is
that codebase's structure, not a wire difference; the committed real K=18 and K=26 sync-buffer
records carry `0x80` at inner `[2]` too, so the command byte cannot select a decoder. Maverick
routes every type-47 record, live or replayed, through the one version-keyed path. The secondary
HR stays unadmitted even for live parity: it would ride the same `heart_rate` stream as the
primary and skew every downstream statistic, so admitting it needs its own stream kind and an ADR.
[WRS]

R22_REALTIME (packet `0x10`): battery percentage is a direct `u8` in `0..=100`, and HR is a `u16`
of milli-bpm divided by 10. [ONE, Rust repo]

EVENT (packet 48), inner-relative like everything above: the event number is the inner command
byte `[2]`, the timestamp `u32` sits at inner 4 (a real RTC value, unlike the realtime device
epoch). The battery event (number 3) carries state of charge as `u16` at inner 13 in deci-percent
(divide by 10), millivolts at inner 17, and a charging flag in bit 0 at inner 22 — again identical
across generations once counted from the inner record (the old gen4 6/8/17/21/26 and "gen5 add 4"
numbers were frame-relative). Known event numbers `[WRS]`: 3 battery level, 7/8 charging on/off,
9/10 wrist on/off, 14 double tap, 17 temperature level, 23 BLE bonded, 33/34 realtime HR on/off,
57/58 strap/app alarm executed, 60 haptics fired. Gen5 events carry an opaque residual past inner
8 that the sixth source surfaces as hex; gen4 events have none. [XVAL fields, WRS byte positions]

Admission (WHOOP-P5): the generation-specific signed connectors decode the
battery event to a `battery_soc` percent (deci-percent / 10, gated to 0..=100; the millivolts and
charging bit have no stream kind yet and stay unemitted) and wrist on/off to `wrist_state` 1/0,
each at the event's RTC second. Every other number is a state transition with no sample stream and
decodes to nothing.

The standard `2A37` RR conversion is `raw * 1000 / 1024` milliseconds. [XVAL]

`COMMAND_RESPONSE` for get_battery differs by generation: gen4 returns `u16 / 10`, gen5 returns a
direct percent byte. This is a genuine 4.0-versus-5.0 difference, not a decode ambiguity. [ONE,
Swift/Kotlin repo]

## Historical records

Historical records are versioned by the **sequence byte** (inner `[1]`) — the "sequence or subtype
byte" ambiguity resolved by the `[WRS]` real captures (see the inner-payload note above); the subtype
byte `[2]` carries the on-wrist r22 command, not the version. Each version has its own field
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
- **gen5 v20 / v21:** bulk multi-channel deep buffers, both decoded from the sixth source `[WRS]`
  but **unverified against a real fill**, because a full buffer needs the R22 deep-data stream
  running on a worn, subscribed band, and that has not been captured (see the subscription-gating
  note below). v20 is the ~25 Hz 6-channel optical buffer: 25 samples per channel of a 20-bit signed
  ADC count in a 4-byte LE word, at `body[36]`/`body[236]`/`body[1302]`/`body[1502]`/`body[1724]`/
  `body[1924]`, identified by the 2×-green-LED echo anchor (`body[20]` == 2 × `body[17]`). v21 is the
  100 Hz 6-axis IMU buffer: columnar `i16`, accel at `body[17]`/`body[217]`/`body[417]` and gyro at
  `body[629]`/`body[829]`/`body[1029]`, identified by both in-packet sample counts equalling 100.
  Treat both as `[PROV]` until a real fill confirms them. `[WRS]`
- **gen5 v26:** a 24 Hz optical PPG waveform, 24 `i16` LE per record, one record per second. Raw
  ADC, with no invented scale.
- **Unmapped versions:** the upstream RE falls back to the V24 layout, then **rejects** if the
  gravity magnitude or HR is outside a physiological range. Maverick does **not** carry that
  guess-fallback: an unmapped version decodes to a typed `DECODE_UNKNOWN_RECORD_VERSION` and its
  bytes stay raw evidence, because a speculative decode of an unknown layout is exactly the kind of
  plausible-but-wrong reading this document keeps warning about. [ONE, Swift/Kotlin repo]

**Admission status (WHOOP-P3).** The signed WHOOP 4 connector admits three gen4 record decoders, each
range-gated: `gen4_v24` (also v12) — HR `body[14]`, the R-R block `body[15]`/`body[16]`, gravity as
three `f32` from `body[33]` (accepted at `|g|` in `[0.5,1.5)`), the SpO2 red/IR raw ADC pair
`body[61]`/`body[63]` (seq 0/1 on `spo2_raw`), the skin-temp register `body[65:67]` as a raw `u16`,
and respiration `body[73:75]`; `gen4_v5` (also v7/v9) — HR and the R-R block only; `gen4_v25` — the
gravity triplet stored as `i16/16384` at `body[66]`/`body[68]`/`body[70]`. v24 and v25 are backed by
real 4.0 goldens `[WRS]` (`fixtures/records/gen4_v24_v1.json`, `gen4_v25_v1.json`); v5 has no real
capture and is pinned by an invariant round-trip test. The skin-temp raw register is admitted, but
its absolute °C scale stays a deferred per-device learned anchor — the gen4 scale is in `[CONFLICT]`
with no calibration golden (ADR-009), so no temperature is claimed. The `skin_contact`,
`signal_quality`, second gravity triplet, `ppg_*`, `ambient`, and LED-drive fields above stay
unadmitted, and the 4.0 offload path is not yet exercised on our own hardware.

### The MG buffer codec (R20: K=18 and K=26)

The MG does not store Harvard's 93-byte V24 records. It stores R20 records in two inner subtypes,
and the fourth source pinned their layouts against a corpus of nearly two million records. [SERIES]
These are historical-buffer records (packet type 47), not realtime, so they belong to Milestone 5;
they are documented here so the work starts from a map rather than a wall.

Both subtypes open with a common ten-byte body header: a `u32` LE counter at `body[0:4]` that
increments once per record and resets on reboot, a `u32` LE unix timestamp at `body[4:8]`, and a
`u16` LE session marker at `body[8:10]`.

**K=18, the per-second metrics record (109 bytes).** Heart rate is a single `u8` bpm at `body[11]`,
zero when the strap has no optical lock, so the same byte doubles as a validity flag. A secondary HR
sits at `body[26]` and tracks the primary at correlation +0.94. Skin temperature is a raw `u16` LE
register at `body[62:64]` scaled by 0.01 to give degrees Celsius, traceable to the strap's ams AS6221
sensor; the sixth source `[WRS]` reads it as an unsigned register and keeps it only when `raw/100` °C
lands in the physiological `[5, 45)` band, which drops garbage bytes a blind `i16` read would store.
A packed state byte at `body[70]` carries an on-wire sleep state in bits 5–4, decoding to `{0 STILL,
1 WAKE, 2 SLEEP, 3 UP}` (the STILL/SLEEP split is decisive in the data; WAKE versus UP cannot be
told apart from passive captures, so treat that half as [PROV]). SpO2 is a single tri-mode byte at
`body[71]`: real percentages in one range, saturation sentinels with bit 7 set, and low-value
diagnostic codes, so a naive mask reads a `0x08` diagnostic code as "8 %". `body[96]` looks like a
second heart rate and is not, it is an AGC readback whose correlation with real HR is about zero.
[SERIES for these, HIGH confidence on HR/skin-temp/sleep-state, [PROV] on the rest]

Two of the fourth source's labels here are **refuted** by the fifth, which correlated them against
worn-session ground truth and found no signal. Do not re-chase either. The "inverted motion" byte at
`body[104]` (frame offset 115) reads a float-mantissa byte that averages about 140 in both sleep and
wake and does not discriminate state; there is no motion byte at `body[104]`. And the "big-endian
`f32` fusion channels" at `body[33:45]` are a byte-misaligned view of the little-endian gravity floats
at `body[45]`/`49`/`53`, not a separate channel: read that way the first "channel" tracks HR at only
+0.11, not the +0.42 the fourth source claimed. [FIELD refutes [SERIES]] These are textbook
plausible-but-wrong readings, exactly what this document keeps warning about, caught only because the
fifth source could hold the bytes against a real body.

The sixth source complicates the gravity offset specifically. On its committed real worn 5.0/MG frame
(`fixtures/records/r20_k18_v1.json`), the little-endian gravity triplet reads clean at `body[34]`
(`inner[37]`), |g| ≈ 1.01, while `body[45]` reads all zeros — the opposite of the fifth source's
`body[45]` placement just above, on the same version-18 record. The two were captured on different
bands and possibly different firmware, so treat the gen5 v18 gravity offset as `[CONFLICT]` between
`body[34]` [WRS] and `body[45]` [FIELD]. [WRS] is currently favoured, because a committed real frame
decodes to unit gravity at `body[34]` and to zeros at `body[45]`; the gravity-admission work reads
`body[34]` and this flips on our own hardware. [WRS] / [CONFLICT]

Residual K=18 bytes that are decoded but not pinned, meaning not stored and not asserted: a
`cardiac_flags` byte at `body[33]` (see the ECG section for what it is and is not), `rr_packed` at
`body[38]`, `cardiac_status` at `body[40]`, `step_cadence` at `body[59]`, status words at
`body[75]`/`77`/`79`, and an `f32` at `body[113]`. [FIELD] Do not build features on any of them.

One open lead worth a capture rather than a guess: a dense unmapped field at inner offset 27 of a
frame-35 record tracked sleep at +0.73 across one night (71 asleep, 12 wake). It is not in the field
map and not yet identified. [FIELD]

**K=26, the raw-PPG burst (73 bytes).** After the ten-byte header and a two-byte per-burst index at
`body[10:12]`, `body[16:64]` holds 24 `i16` LE photodiode samples, one record per second, giving a
24 Hz wire rate confirmed across 2,332 bursts (sample count equalled duration in seconds times 24,
with no exceptions). 24 Hz is coarse for PPG: a 41.7 ms sample period quantises beat timing to about
±21 ms, which alone floors RMSSD around 30 ms even with flawless peak detection. That floor is a
constraint to design around, not a bug to fix. [SERIES]

**Admission status (WHOOP-P2, extending M5-P4).** The signed WHOOP 5 connector admits, from
K=18, the full corpus-pinned field set, each range-gated so a wrong offset yields nothing: the primary
HR at `body[11]` (zero is the no-lock sentinel); the R-R intervals (count `body[12]` clamped to four,
values `body[13..]`, zero slots dropped); gravity as three `f32` from `body[34]`, accepted only at
finite `|g|` in `[0.5, 1.5)`; skin temperature at `body[62:64]` as a raw `u16` register kept only in
the `[5, 45)` °C band; the sleep-only tri-mode SpO2 percent at `body[71]` kept only in `70..=100`
(emitted as the new `spo2_percent` stream, ADR-014); cumulative steps at `body[46]`; the coarse
activity class at `body[52]` kept only in `{0,1,2}` (the new `activity_class` stream, ADR-014); the
packed sleep state in bits 5–4 of `body[70]`; and the PPG confidence `signal_quality` `u8` at
`body[29]`. From K=26, the 24-sample photodiode burst as raw ADC. The empirical `signal_flags`
bitfield at `body[22]` has no clean stream kind and stays recorded but unemitted; the secondary HR
and everything marked residual/refuted/[PROV]-only above stay unadmitted, and unknown version bytes
produce a typed `DECODE_UNKNOWN_RECORD_VERSION` rather than a fallback decode.

**Admission status (WHOOP-P4, the deep buffers).** `gen5_v21` and `gen5_v20` are admitted as
`[PROV]`/UNVERIFIED (ADR-015), pinned by synthetic invariant tests because no real fill has been
captured. v21 emits the 100-sample 6-axis IMU — accelerometer as `imu`, gyroscope as `gyro`, both
`seq = sample*3 + axis`, raw `i16` — gated on both sample counts equalling 100. v20 emits the
6-channel × 25-sample optical buffer as `optical_raw`, `seq = channel*25 + sample`, 20-bit
sign-extended `i32`, gated on the 2×-green anchor. **Routing caveat:** Maverick routes these by the
manifest version byte (20/21), whereas the upstream RE tries the IMU layout first because a real deep
buffer's version byte was once seen colliding with v18. With no real deep capture to settle which
version byte these carry, the internal count/anchor gates are the safety net — a mis-routed frame
that fails them decodes to nothing. Revisit the routing when a real fill arrives.

**The off-by-eleven trap.** Each MG record arrives inside an 8-byte frame header plus a 3-byte inner
prefix (type, sequence, subtype) before the body, so a position measured from the frame start is 11
bytes ahead of the same position measured from the body start. An early field map placed SpO2 at
frame offset `0x52` and landed on a skin-temperature byte; `0x52 − 11 = body[71]` is the real SpO2
byte. Every offset above is body-relative. Maverick already counts from the inner payload rather
than the frame (the `mav-codec` field layouts are offsets into `payload`, where `payload[0]` is the
packet type), so the manifest offset for `body[N]` is `N + 3`.

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
- The fourth source reads the same field (at `body[65:66]` of the 93-byte V24 record) as `raw * 0.04`,
  which lands around 31–36 °C on the bicep, and warns that a nearby offset (`body[69:70]`) is LED
  drive current, not temperature, so it tracks perfusion and impersonates a temperature signal. That
  mislabel is exactly the sort of plausible-but-wrong reading this document keeps flagging. [SERIES]

The three slopes (0.033, 0.05, 0.04 °C per count) are close enough to agree that the field is skin
temperature and far enough apart that the absolute value is not settled. All sources say the gen4
absolute figure is provisional and only deviation from the device's own baseline is defensible.
[PROV / CONFLICT]

The design consequence for Maverick is set out in [../connectors.md](../connectors.md), and it is
worth restating here because it drives a piece of the architecture. Skin temperature is modelled as
a per-device learned anchor plus slope, which makes the fixed-anchor approach a special case of the
learned one, stored in the per-device key-value table, and surfaced as a deviation from personal
baseline rather than as an absolute thermometer reading, until there is a hardware calibration.
This is exactly the kind of device-local state a static manifest cannot hold. Under ADR-017 the
WHOOP 4 connector learns it and persists it through connector-scoped state actions; generic core
never learns WHOOP calibration rules.

## Raw sensor streaming

- **Type-43 raw realtime (R10/R11):** controlled by command 63. Send `[0x00]` on connect to stop the
  flood, or the strap will keep streaming raw samples. A real type-43 live raw capture is still
  pending; it needs a research tap alongside the bond, which has not been set up. [FIELD]
- **gen5 raw 6-axis IMU offload:** roughly a 1244-byte buffer of 100 accelerometer `i16` and 100
  gyroscope `i16`, columnar, at 100 Hz. A live IMU request (command 106) is refused by the firmware;
  the offload path is what actually yields data. [ONE, Swift/Kotlin repo]
- **PPG:** gen5 v26 gives 24 Hz; gen4 raw optical is roughly 437 Hz on a single green channel as
  `s24` LE. [ONE each]

### The R22 deep stream may be unreachable over BLE alone

This is the fifth source's sharpest finding, and it changes what the enable sequence is worth. On a
real worn MG, the fifteen or sixteen `SET_CONFIG` feature-flag writes are accepted byte for byte and
fully acknowledged, exactly as the third source captured. But acceptance is not activation: across
five worn sessions of up to 4.1 hours after a full ACK, the strap produced **zero** deep-data frames
(no type 51–56), only plain type-40 heart rate. The deep optical stream appears to be
subscription- or server-gated, not BLE-flag-gated, so on a strap without an active WHOOP subscription
the deep buffer may simply be unreachable no matter what flags are set. [FIELD]

For Maverick this is a caution, not a decode. The `enable_sequence` in the connectors is still worth
carrying, because the flags are real and correct, but the connector documents that setting them does
not by itself guarantee deep data; the honest expectation on a no-subscription strap is standard
heart rate and whatever the ordinary sync buffer holds, not the R22 firehose. Whether a subscription,
a specific server handshake, or a research bond unlocks it is an open `[HW]` question.

### There is no live optical stream on the gen5 straps

This is the finding that shapes the whole gen5 strategy. On the MG (and the 5.0, which shares its
firmware) there is no live raw-optical feed to subscribe to. The command that should start one,
`ENABLE_OPTICAL_DATA` = `0x6B` (107), is present in the app but not implemented in the strap: it is
neither refused nor acknowledged, it is silently discarded, and two independent investigations
reached that conclusion separately. [JUDES and SERIES agree] The realtime commands that do respond,
`0x3F` (63, `SEND_R10_R11_REALTIME`) on gen5 and `0x6A` (106, `TOGGLE_IMU_MODE`) on gen4, stream
six-axis inertial data plus a single HR byte, not optical. [SERIES]

The consequence is that every gen5 signal, heart rate, HRV, SpO2, skin temperature, sleep state, and
the raw PPG, comes out of the historical sync buffer or not at all. The same `0x2F` (47) replay
mechanism that carries backfill on the 4.0 is the carrier for the MG's per-second and burst records
too; there is no separate realtime channel behind them. For Maverick this means the gen5 realtime
path in the M1 manifest (packet 40) is really the standard heart-rate profile plus buffered replay,
and that the historical pipeline (Milestone 5) is not a nice-to-have for the gen5 straps, it is the
only way in.

### ECG on the MG

The MG is the medical-grade variant, and it adds a single-lead ECG and a dedicated skin-temperature
sensor on top of the optical front-end. Its firmware is byte-identical to the 5.0's; the product
difference is selected in software through config keys, so the ECG and skin-temperature paths are
the only MG-only behaviour at the BLE level. [SERIES] The hardware behind the bytes, worth knowing
because every decoded field traces to one of these: an Ambiq Apollo4 Blue MCU, a Maxim MAX86176
optical front-end, a TDK ICM45686 six-axis IMU, and an ams AS6221 skin-temperature sensor. [SERIES]

None of the five sources has decoded the ECG waveform. It is not in any mapped record, and no source
has found the command or the record type that carries it. So the honest state is: **the MG has ECG
hardware, and reading its waveform over Bluetooth is unsolved as far as we know.** Finding it will
need our own MG, an on-wrist ECG session captured while the official app records one, and the same
labelled-capture, corpus-pinning method the other sources used on the optical records. This is an
`[HW]` item on the hardware checklist, not something to design a decoder for now.

The fifth source did pin down where it lives, which narrows the search. Genuine AFib and rhythm
detection is on WHOOP's ECG/HeartKey path (the firmware carries strings like `HK … Afib %d … #ECG %d`),
it is electrode-gated, and it is MG-only: the 5.0 firmware explicitly blocks it (`ECG Control not
supported on Goose hardware`, where Goose is the PPG-only 5.0). [FIELD] So the 5.0 will never yield
ECG no matter what, and on the MG the ECG path is gated behind the electrodes and its config key.

A related correction, because it is the kind of claim that would mislead a medical reading of this
data: **there is no cardiac or arrhythmia detector on the 5.0's optical path.** The K=18 bytes once
labelled `cardiac_flags` (`body[33]`) and `cardiac_status` (`body[40]`) are not an arrest or AFib
alarm. `body[33]` is a PPG signal-processing status bitfield (AGC, HR-channel switch, wear detect,
SNR) and `body[40]` is a signal-confidence byte; neither is an event code, and across 590 event and
pairing frames none matched any arrhythmia pattern. [FIELD] Do not surface either byte as a health
alert.

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

Metadata kinds are 1 START, 2 END (which carries the cursor data to acknowledge), and 3 COMPLETE.
On a real 5.0/MG END frame (`[WRS]`, fixtures/control/gen5_history_end_v2.json) the body is: the
record unix `u32` at inner 3, and the 8-byte end_data — trim cursor `u32` + next `u32` — at inner
13..21. The acknowledgement echoes exactly those eight bytes behind the b3 marker; echoing the
whole body would hand the strap the record unix as a cursor. An earlier synthetic fixture put the
cursor at body start, which made a whole-body echo look correct.

### The safe-cursor invariant

The Swift/Kotlin repo's backfiller is careful in a way worth copying exactly. The read cursor advances
only after the corresponding chunk has been durably stored on the phone. The order is: decode,
persist durably, optionally enqueue the raw batch, then acknowledge the cursor. A persist failure
blocks all further acknowledgements, so the cursor can never advance past history that was not
stored. `HISTORICAL_DATA_RESULT` does not delete the buffered records; only the separate
`FORCE_TRIM` command does that, and Maverick does not issue it automatically. [ONE, Swift/Kotlin
repo, strengthened by the sniffed command distinction in SERIES]

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
mav PR #163, and its iOS parity commit is in the current Maverick git history. In Maverick it is an
invariant test in `mav-timeline`, described in [../pipeline.md](../pipeline.md).

## The hardware checklist

When the straps arrive, this document becomes a checklist. Every [CONFLICT] is resolved against a
live capture: the high-frequency-sync opcodes, and the gen4 hello opcode (35 versus 145). Every
[PROV] value is calibrated or confirmed: the gen4 skin-temp scale (now three candidate slopes), the
SpO2 constants and the MG's tri-mode SpO2 byte, the resp scale, the guessed gen5 v18 offsets, and
the WAKE-versus-UP half of the MG sleep-state enum. Every [HW] assumption is checked, MG capability
parity with the 5.0 among them.

Several items need a specific capture rather than a spot-check. **The MG ECG** is unsolved in every
source; finding it needs an on-wrist ECG session recorded while the official app captures one, with
the electrodes engaged, then the labelled-capture method against stored raw bytes. **The MG sleep
SpO2** is the same shape of gap: the strap computes a 0–100 SpO2 scalar on-wrist during sleep and
exports it, but every capture anyone has drained so far was taken awake, so the byte that carries it
has not been seen changing. A real multi-hour sleep drain, diffed at `body[71]` (`inner[74]`) against
the console SpO2 oracle across sleep versus wake, is what pins it. **The R22 deep stream** is the
open question the fifth source raised: the flags ACK but the deep frames never came on a
no-subscription strap, so the checklist item is to find whether a subscription, a server handshake,
or a research bond is what actually unlocks it. **The v20 optical layout** is unknown and **the v21
IMU fill** is unverified, and both wait on that same deep stream running on a worn band. And the
**frame-35 offset-27 sleep lead** (+0.73 across one night) wants a labelled multi-night capture to
confirm or drop. All of these are why the connector system is built to run without hardware now and
absorb these facts as data later, rather than blocking on devices we do not yet have.

Every [XVAL] fact is still spot-checked, because agreement between reverse-engineered sources is a
strong prior, not a measurement. Fixtures are regenerated from our own captures, and the tags in
this file flip to hardware-verified as each item is confirmed.
