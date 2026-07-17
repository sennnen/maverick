# Platform fixtures

The canonical `host-snapshot/v1` bytes the native apps decode, pinned at the app/core seam.

- `host_snapshot_v1.expected.json` — the exact canonical JSON and hash `HostRuntime::host_snapshot`
  returns after the `realtime_rr_prv_v1` capture is fed chunk by chunk under a pinned config
  (Europe/London, app `0.1.0`/`fixture`) and a pinned observation time. Produced by the code, so it
  is a regression pin, not an oracle: the PRV numbers inside it must keep matching
  [`../replay/realtime_rr_prv_v1.expected.json`](../replay/realtime_rr_prv_v1.expected.json), which
  was generated independently.

Three tests hold this file in step, one per language:

- Rust: `mav-engine` `host_snapshot_reproduces_the_platform_fixture` (also the generator, with
  `MAV_BLESS=1`);
- Kotlin: `MavSnapshotDecoderTest` decodes the `json` field and asserts the presented values;
- Swift: `MavSnapshotTests` decodes the same bytes and asserts the same values.

Changing what the core puts in the snapshot regenerates this file and fails both platform decode
tests until their expectations move too — that is the PL-P7 parity seam working as intended. Never
edit it by hand (see [skills/golden-fixtures](../../skills/golden-fixtures/SKILL.md)); regenerate,
then eyeball the values against the replay fixture before trusting them.
