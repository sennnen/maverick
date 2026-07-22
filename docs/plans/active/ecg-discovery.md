# ECG-discovery lane — finding the MG's waveform on the wire

The WHOOP MG carries a single-lead ECG. **No source has decoded it.** It is in no mapped record, no
source has found the command or the record type that carries it, and we have no official app or
subscription to observe doing it properly. This lane is genuine reverse-engineering on our own
hardware, and it is written down as it goes because the failures are as informative as the wins.

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

**Status: open.** Two runs, no waveform, three infrastructure defects fixed that would have hidden
one, and one hardware-verified identity readout. The next step is physical.
