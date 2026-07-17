# Record fixtures

Historical-buffer records (packet type 47) paired with the exact samples the admitted M5-P4
decoders must produce. Both are **synthetic and [PROV]**: built to the corpus-pinned body offsets
in [docs/protocol/whoop.md](../../docs/protocol/whoop.md) (fourth/fifth source, ~2M records) with
an independent Python implementation, never with the code under test. Regenerate from a real
capture in the hardware epoch.

- `r20_k18_v1.json` — the MG per-second metrics record. Expected output is exactly heart rate
  (`body[11]`), skin temperature (`body[62:64]`, i16 LE centidegrees), and the packed sleep state
  (bits 5–4 of `body[70]`, stored as the raw wire state). The residual and refuted bytes carry
  non-zero noise, and the tri-mode SpO2 byte holds diagnostic code `0x08`, so the test proves the
  decoder ignores everything unadmitted — that byte must never surface as "8 %".
- `r20_k26_v1.json` — the MG raw-PPG burst: 24 i16 LE photodiode samples at `body[16:64]`, raw ADC
  with no invented scale, spanning negative and positive values so signedness is pinned. Each
  sample keeps the record's second and its in-burst index as `seq`; no sub-second timestamps are
  invented.

Unknown versions (v20, v21, V24, …) have no fixtures here on purpose: they are unadmitted, decode
to `DECODE_UNKNOWN_RECORD_VERSION`, and their bytes stay raw evidence. Never edit these files by
hand (see [skills/golden-fixtures](../../skills/golden-fixtures/SKILL.md)).
