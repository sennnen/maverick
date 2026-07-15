---
name: connector-authoring
description: >
  How to add a new device to Maverick: write connectors/<device>/manifest.json for everything
  static, add a small DeviceCodec only for logic that data cannot express, and keep every core
  crate untouched. Load this when adding a strap or other device, writing or changing a
  connector manifest, or when a task says "add support for <device>".
---

# Adding a device

The whole point of the connector layer is that a new device is data, not core surgery. So the
rule comes first, before any step:

**Adding a device must not require editing any crate under `core/`.** If you find yourself
opening `mav-codec`, `mav-frame`, or any other core crate to make a device work, stop. What
you need is either missing from the manifest schema or missing from the DeviceCodec contract,
and that is a change to `docs/connectors.md` backed by an ADR, decided deliberately, not a
quiet edit buried in a device connector.

`docs/connectors.md` is the authority for the manifest schema and the codec contract. Read it
before you start; the notes below are the shape, not the specification.

## Steps

1. **Write `connectors/<device>/manifest.json`.** This holds everything static about the
   device, all as data: identity and how to disambiguate it at scan time, GATT service and
   characteristic UUIDs, frame parameters, the packet-type map, field layouts, unit
   conversions, the record versions it emits, and its sensor configs. Most devices are
   nothing more than this file.

2. **Add a `DeviceCodec` crate only if data cannot express the logic.** Some things a manifest
   genuinely cannot describe: a stateful handshake, a decode that needs memory of earlier
   frames, or a learned per-device value like the WHOOP 4.0 skin-temperature anchor, which is
   fitted per band rather than fixed. Those go in a small codec. The codec is boxed in by its
   interface: it receives bytes, its own manifest, and a per-device key-value store for learned
   state, and it returns frames or samples. It cannot reach storage, the network, analytics, or
   any other device. If your codec wants any of those, the design is wrong.

3. **Prove it with a fixture.** Take a capture from the device, generate a golden fixture
   through `skills/golden-fixtures`, and run it through the pipeline with `mav-replay`. A
   device without a fixture is not really supported, just asserted.

The `mock` connector exists to keep this honest: it is a deliberately odd fake device with a
different frame format and a codec that needs per-device state, and it streams through the
untouched pipeline. If adding a real device tempts you toward a core edit, the mock is the
proof that it should not be necessary.
