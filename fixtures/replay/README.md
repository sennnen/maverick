# Replay fixtures

A capture, a manifest, and the snapshot they produce, for `mav-replay` and the pipeline test.

- `realtime_hr_v1.capture.json` — three gen5 REALTIME_DATA frames carrying heart rates 58, 61, 63
  bpm at consecutive seconds. **Synthetic and [PROV]:** the frames were built to the documented
  gen5 layout with an independent implementation (Python, zlib CRC-32, CRC-16/Modbus), not with the
  code under test, so the test that decodes them can genuinely fail. There is no real capture in
  this repository yet; regenerate this from a live capture in the hardware epoch.
- `realtime_hr_v1.manifest.json` — a minimal realtime manifest for the fixture. It is a test
  fixture, not a shipped connector (the real device manifests live in the separate connectors repo,
  see [ADR-011](../../docs/adr/ADR-011.md)); it exists so the core's replay test stays
  self-contained and hardware-free.
- `realtime_hr_v1.expected.json` — the snapshot and canonical hash the replay must reproduce. It is
  produced by the code, so a change to any algorithm version changes this file, which is exactly the
  regression signal a golden fixture is for. Produced with the algorithm versions recorded inside
  it; a version bump means a new expected file, per [testing.md](../../docs/testing.md).

`realtime_rr_prv_v1` exercises the ledger-backed gen5 packet-40 RR layout: count at offset 17 and
`u16` little-endian intervals from offset 18. Its sequence is `800, 800, 850, 790, 900, 0, 50` ms.
The decoder drops the zero placeholder, SQI rejects 50 ms, and the two equal 800 ms intervals remain
distinct through their `seq` values. Frame CRCs and the expected time-domain values were generated
with an independent Python implementation, not Maverick.

`mixed_history_v1` is the M5-P5 canonical-merge fixture: five gen5 chunks mixing packet-40
realtime seconds with packet-47 K=18 history records, including a duplicated historical chunk
(must store once) and a stale-clock record (1970 timestamp: the wall time falls back to the
capture wall, the raw device time is preserved, and the sample carries the implausible-timestamp
reason). Synthetic [PROV], built with an independent Python implementation; the expected snapshot
is produced by the code and its values are checked by hand in the file's evidence note.
