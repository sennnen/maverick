# Historical-control fixtures

Control frames — command responses and burst metadata — paired with the exact typed value
`mav-codec::control::decode_control` must produce (M5-P2). Unless a fixture says otherwise they
are **synthetic and [PROV]**: built to the documented envelopes and the [XVAL] inner header
(`[0]` packet type, `[1]` sequence, `[2]` command/kind, `[3..]` body) with an independent Python
implementation (zlib CRC-32, CRC-16/Modbus, CRC-8 poly 0x07), never with the code under test.
Regenerate each synthetic one from a live capture in the hardware epoch.

- `gen4_command_response_v1.json` — SEND_HISTORICAL_DATA answered with the sniffed pending byte
  `0x02` in gen4 framing.
- `gen5_command_response_v1.json` — GET_DATA_RANGE answered ok (`0x01`) in gen5 framing.
- `gen5_history_start_v1.json` — METADATA kind 1.
- `gen5_history_end_v2.json` — METADATA kind 2, a **real 5.0/MG capture** (`[WRS]`, imported from
  tanarchytan/whoop-rs): the acknowledgement is exactly the 8-byte end_data (trim cursor + next
  `u32`) at inner 13..21, never the whole body, whose leading bytes are the record unix. The
  superseded `gen5_history_end_v1.json` stays as the record of the earlier whole-body echo, which
  a synthetic short-body frame made look correct; no test reads it.
- `gen5_history_complete_v1.json` — METADATA kind 3, the strap-only end of the exchange.

`expected.record_count` is `null` everywhere: no admitted source pins a record count inside `END`,
so the decoder yields `None` until a capture proves one. Never edit these by hand (see
[skills/golden-fixtures](../../skills/golden-fixtures/SKILL.md)).
