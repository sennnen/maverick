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
