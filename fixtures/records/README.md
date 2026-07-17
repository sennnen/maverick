# Record fixtures

Historical-buffer records (packet type 47) paired with the exact samples the admitted record
decoders must produce. All are **real captures `[WRS]`**, imported from `tanarchytan/whoop-rs` and
committed with only biometrics + timestamp (no serial/name/token) — three WHOOP 5.0/MG and two
WHOOP 4.0. They replaced the earlier synthetic `[PROV]` fixtures, which encoded the layout version
in the wrong inner byte (`[2]` instead of `[1]`) and so did not match any real frame — the exact
drift the "regenerate from a real capture" rule exists to catch. Each fixture's `wire_format` field
selects the reassembler and manifest. The offsets they exercise are the corpus-pinned ones in
[docs/protocol/whoop.md](../../docs/protocol/whoop.md); the expected samples were computed by an
independent decode, never the code under test. They still owe a check on our own strap in the
hardware epoch — the 4.0 offload path especially, which is not yet exercised on our own hardware.

- `r20_k18_v1.json` — a real worn MG per-second metrics record (version 18 at inner `[1]`, on-wrist
  r22 command `0x80` at inner `[2]`). Expected output is the full admitted field set, each
  range-gated: heart rate (`body[11]`), the two R-R intervals (count `body[12]`, values `body[13..]`),
  gravity (three f32 from `body[34]`, accepted at |g| ≈ 1), skin temperature (`body[62:64]`, raw u16
  register kept in the 5–45 °C band), steps (`body[46]`), activity class (`body[52]`), the packed
  sleep state (bits 5–4 of `body[70]`), and signal quality (`body[29]`). The capture is awake, so its
  sleep-only tri-mode SpO2 byte (`body[71]`) is 0 and stays unadmitted — proving the 70..=100 gate on
  a real frame — the empirical `signal_flags` bitfield has no stream kind and is not emitted, and
  every other residual/refuted byte produces nothing. See ADR-014 for the two added stream kinds.
- `r20_k26_v1.json` — a real MG raw-PPG burst (version 26): 24 i16 LE photodiode samples at
  `body[16:64]`, raw ADC with no invented scale, spanning negative and positive values so signedness
  is pinned. Each sample keeps the record's second and its in-burst index as `seq`; no sub-second
  timestamps are invented.
- `gen4_v24_v1.json` — a real worn WHOOP 4.0 DSP record (version 24): HR (`body[14]`), two R-R
  intervals (`body[15]`/`body[16]`), the gravity triplet (three f32 from `body[33]`), the SpO2 red/IR
  raw ADC pair (`body[61]`/`body[63]`, seq 0/1), the skin-temp register (`body[65:67]`, raw u16 — the
  absolute °C scale is a deferred per-device anchor, so no temperature is claimed), and respiration
  (`body[73:75]`). `[WRS]`; the 4.0 offload is not yet on our own hardware.
- `gen4_v25_v1.json` — a real WHOOP 4.0 v25 (PPG-buffer) record: gravity only, stored as `i16/16384`
  at `body[66]`/`body[68]`/`body[70]`, gated to |g| ≈ 1. v25 carries no per-second HR.

The gen4 v5/v7/v9 generic record has no real capture yet, so it is pinned by an invariant round-trip
test in `record_fixtures.rs` rather than a golden here. Unknown versions (v20, v21, …) have no
fixtures on purpose: they are unadmitted, decode to `DECODE_UNKNOWN_RECORD_VERSION`, and their bytes
stay raw evidence. Never edit these files by hand (see
[skills/golden-fixtures](../../skills/golden-fixtures/SKILL.md)).
