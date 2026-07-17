# Record fixtures

Historical-buffer records (packet type 47) paired with the exact samples the admitted record
decoders must produce. Both are now **real 5.0/MG captures `[WRS]`**, imported from
`tanarchytan/whoop-rs` and committed with only biometrics + timestamp (no serial/name/token). They
replaced the earlier synthetic `[PROV]` fixtures, which encoded the layout version in the wrong
inner byte (`[2]` instead of `[1]`) and so did not match any real frame — the exact drift the
"regenerate from a real capture" rule exists to catch. The offsets they exercise are the
corpus-pinned ones in [docs/protocol/whoop.md](../../docs/protocol/whoop.md); the expected samples
were computed by an independent decode, never the code under test. They still owe a check on our own
strap in the hardware epoch.

- `r20_k18_v1.json` — a real worn MG per-second metrics record (version 18 at inner `[1]`, on-wrist
  r22 command `0x80` at inner `[2]`). Expected output is exactly heart rate (`body[11]`), skin
  temperature (`body[62:64]`, i16 LE centidegrees), and the packed sleep state (bits 5–4 of
  `body[70]`, stored as the raw wire state). The capture is awake, so its tri-mode SpO2 byte is 0 and
  stays unadmitted, and every other residual/refuted byte produces nothing — the test proves the
  decoder emits those three streams and no more.
- `r20_k26_v1.json` — a real MG raw-PPG burst (version 26): 24 i16 LE photodiode samples at
  `body[16:64]`, raw ADC with no invented scale, spanning negative and positive values so signedness
  is pinned. Each sample keeps the record's second and its in-burst index as `seq`; no sub-second
  timestamps are invented.

Unknown versions (v20, v21, V24, …) have no fixtures here on purpose: they are unadmitted, decode
to `DECODE_UNKNOWN_RECORD_VERSION`, and their bytes stay raw evidence. Never edit these files by
hand (see [skills/golden-fixtures](../../skills/golden-fixtures/SKILL.md)).
