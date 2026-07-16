# Mock device wire format

The mock is a fake device whose only job is to test the connector abstraction by being deliberately
awkward. Every choice here differs from WHOOP on purpose, to force an assumption that the WHOOP
formats could have quietly baked into the core out into the open. It is not a real device and never
ships; it lives in the core repo (not the connectors repo) because it is a test fixture for the
abstraction, not a distributable connector. See [../../docs/connectors.md](../../docs/connectors.md)
and the M2 plan.

## Frame

```
[0]        0x5A            start-of-frame (WHOOP is 0xAA)
[1..=2]    payload length  u16 BIG-endian (payload bytes only, excluding header and trailer)
[3..]      payload
[last]     CRC-8(payload)  one trailing byte, polynomial 0x07, init 0x00 (mav-frame's crc8)
total    = payload length + 4
```

No padding. The CRC-8 is the same function `mav-frame` already ships; the mock does not reimplement
it.

## Payload

```
[0]   record type
[1]   sequence  u8
[2..] body
```

Record type `0x00`, keyframe: body is `timestamp` u32 **big-endian** (seconds), then `base_hr` u8
(the absolute heart rate in device units), then `anchor` i8 (the final body byte).

Record type `0x01`, delta: body is `timestamp` u32 big-endian, then `delta` i8 (the signed change
from the previous decoded heart rate). A delta record is undecodable without the previous value,
which is the designed-in statefulness.

Device units to bpm: `bpm = device_units + anchor`, where `anchor` is a small per-device calibration
offset carried in the keyframe and persisted to the per-device key-value store. This mirrors the
WHOOP 4.0 learned skin-temp anchor, the exact case that justified giving the `DeviceCodec` a KV
handle in the first place.

## Deliberate differences, and the assumption each one targets

| difference | assumption it targets |
|---|---|
| SOF is `0x5A`, not `0xAA` | that the start-of-frame byte is a constant |
| length is big-endian | that multi-byte lengths are little-endian like WHOOP |
| a single CRC-8 trailer, no header CRC | that a frame has a 4-byte CRC-32 and a header CRC |
| no padding | that payloads pad to a 4-byte boundary (the gen5 rule) |
| delta encoding needs the previous value | that a record decodes statelessly, on its own |
| per-device anchor learned into the KV store | that a manifest holds everything a device needs |

## What the manifest can express, and what falls to code

The manifest can express the identity, the capability set (exactly `HeartRate`, deliberately no
`RrInterval`, so M3 can prove capability negotiation hides recovery), the packet map, and the field
layouts. What falls to the codec is the delta decode that needs memory across frames and the anchor
that is learned and persisted, which is precisely what a `DeviceCodec` exists to hold.

## The finding: the current schema cannot express this device's framing

Building the mock surfaced a real limit in the M1 design, which is exactly what M2 is for. In M1 the
core frames incoming bytes with a reassembler chosen by the manifest's `frame.wire_format`, a closed
enum of `gen4` or `gen5`, and only then hands the `DeviceCodec` an already-reassembled `RawFrame`.
The mock's framing is none of gen4 or gen5: a different SOF, a big-endian length, a CRC-8 trailer and
no header CRC. So two things are true at once, and both block the mock:

- The manifest cannot declare the mock's frame format. `frame.wire_format` accepts only `gen4` or
  `gen5`, and `Manifest::validate` rejects anything else, so `connectors/mock/manifest.json` cannot
  even load while its wire format is honest.
- The `DeviceCodec` cannot own its framing either, because the trait receives a `RawFrame` that the
  core reassembler already produced; a codec never sees raw bytes.

This is not a thing to work around by pretending the mock is gen5. It is the abstraction failing
early, which is the milestone doing its job. The resolution is
[ADR-012](../../docs/adr/ADR-012.md): frame parameters become manifest data, so a device's framing
is declared, not hardcoded, and gen4, gen5, and the mock all become configurations of one
reassembler. The rest of this format (the delta decode and the anchor) is genuine codec work and is
unaffected; only the framing waits on that refactor.
