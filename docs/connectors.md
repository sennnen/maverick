# Connectors

A connector is how Maverick knows how to talk to one family of device. It is a `manifest.json` file
and, when the device needs logic that a file cannot express, a small `DeviceCodec`. The rule that
governs the whole design is short: adding a device is one manifest plus at most one small codec, and
zero edits to the core. If adding a device requires touching a crate under `core/`, the abstraction
has failed and the fix is to widen the manifest or the codec contract, not to special-case the new
device inside the pipeline.

## Why not purely declarative

The tempting version of this design is a pure manifest: describe every device as data, decode
everything by interpreting that data, and never write device-specific code at all. Maverick does
not do that, because the prior codebases showed it does not hold. Some device behaviour is stateful
(authentication and handshakes), and some decoding needs memory or a value that has to be learned
from the device over time. A static file cannot carry a value that does not exist until the device
has been worn for a while.

The clearest example is skin temperature on the WHOOP 4.0. The gen5 straps report skin temperature
as centidegrees, so degrees Celsius is `raw / 100`, and that is a fixed conversion a manifest states
directly. The gen4 is not so kind. The two surveyed codebases disagree on its absolute scale: one
uses a fixed anchor (`delta_c = (raw - 930) / 30`, with raw 930 read as 33 °C), and the other uses
a per-device affine fit (`°C = 33.0 + (raw - anchorRaw) * 0.05`, where `anchorRaw` defaults to 826
but is learned from the worn-band median for that specific device). Both codebases say plainly that
the gen4 absolute figure is provisional and that only deviation from the device's own baseline is
defensible until there is hardware calibration. See [protocol/whoop.md](protocol/whoop.md) for the
tags on these facts.

That learned per-device anchor cannot live in a manifest, because it is different for every physical
strap and it does not exist until the strap has been worn. It is a value the connector computes and
remembers. So the connector contract has two parts: a manifest for everything static, and a codec
for the small amount of logic and state that static data cannot represent. Maverick models skin
temperature the general way, as a per-device learned anchor and slope (which makes the fixed-anchor
approach a special case), stored in the per-device key-value table and surfaced as a deviation from
personal baseline rather than an absolute thermometer reading.

## What goes in the manifest

`manifest.json` holds everything about a device that is static, meaning it is true for the whole
device family and known before any strap is ever connected:

- **Identity.** The device family name and the model strings that map to it, including how to
  disambiguate models that are indistinguishable at scan time. (WHOOP 5.0 and MG share a service
  UUID and are told apart only by a registry or model string; an unknown model string defaults to
  gen5.)
- **GATT.** Service, command, and notify characteristic UUIDs, and any standard GATT characteristics
  the device also exposes.
- **Frame parameters.** The start-of-frame byte, the header layout, the CRC families and their
  parameters, the padding rule, and the maximum frame size. `wire_format` is `gen4`, `gen5`, a
  `custom` spec described as data (ADR-012), or `unframed` — a standard GATT characteristic whose
  notification values arrive whole, with no header and no CRC, so the reassembler runs in
  passthrough (PL-P8).
- **Packet map.** The numeric packet types and what each one is (realtime data, historical data,
  event, metadata, command response, and so on).
- **Field layouts.** For each packet type and record version, the offset, width, endianness, and
  meaning of each field.
- **Unit conversions.** The scale and offset that turn a raw field into a physical value, where that
  conversion is a fixed constant.
- **Record versions.** The historical record versions the device emits, keyed by their version or
  subtype byte, each with its own field layout and a maturity note.
- **Codec id.** A device whose decode the DSL cannot fully express names its codec — `codec:
  "whoop"`. The id resolves to a compiled `mav-connector-<family>` crate the edge registered
  (ADR-016); naming `record_versions` or an `event_vocabulary` without naming a codec is a
  validation error, because nothing in the core can decode them.
- **Event vocabulary.** A device whose event packet carries a number byte selecting a per-event
  body layout names one admitted vocabulary — `event_vocabulary: "whoop"` — instead of a layout.
  One number choosing among body layouts is the same DSL-can't-express shape as the standard
  profile, so each vocabulary is a reviewed module in the device's codec crate and the manifest
  can only name it; the name is checked against what the codec admits when the connector
  installs. Admitted mappings decode to samples at the event's RTC timestamp (WHOOP: battery
  state of charge, wrist on/off); event numbers without a stream mapping decode to nothing, like
  control packets (WHOOP-P5).
- **Standard profile.** A pure standards connector (the built-in BLE Heart Rate connector) names
  one admitted profile decoder — `standard_profile: "heart_rate"` — instead of a packet map. The
  Heart Rate Measurement layout is flag-driven, which the layout DSL cannot express, so the
  decoder is a reviewed module in `mav-codec/src/standard.rs` — the one decoder family that stays
  in the core, because a Bluetooth SIG profile is an open standard, not a device family — and the
  manifest can only name it. Standard characteristics carry no device clock; the pipeline stamps
  each sample with the phone-side receive time, unflagged, because that is the honest time of a
  clockless reading.
- **Capabilities and interval source.** The stream kinds the device produces, plus `ppg`, `ecg`, or
  `unknown` for beat-to-beat intervals. This controls whether variability may be labelled optical
  PRV or ECG HRV; the presence of RR alone cannot answer that.
- **Sensor configs.** The commands and parameters used to start, stop, and configure raw-sensor
  streams.

Everything in that list is data, and data is the right home for it because a manifest can be
reviewed, diffed, and validated without reading Rust, and because a wrong offset in a manifest is a
one-line fix that does not risk the pipeline. Where a protocol fact is uncertain, the manifest
records it with the same confidence honesty the protocol document uses; a guessed offset is marked
as such rather than presented as settled.

## What goes in the codec

A `DeviceCodec` holds only the logic that data cannot express. In practice that is a short list:

- Reviewed decoders where one byte selects among body layouts (historical record versions, event
  vocabularies) — dispatch, not data, so the manifest names them and the codec carries them.
- Stateful handshakes and authentication sequences.
- Decodes that need memory across frames.
- Values learned from the device over time, such as the gen4 skin-temp anchor.
- The device's outbound command builders (alarm, haptics), which are opcode tables and body
  layouts no other device shares.

If a piece of behaviour can be written as a manifest field, it must be, and it does not belong in the
codec. The codec is for the residue that genuinely cannot be a table.

A codec is a compiled crate under `core/connectors/`, named `mav-connector-<family>`
(ADR-016). It may depend only on `mav-model`, `mav-frame`, and `mav-codec` — check_deps enforces
this — and it reaches the pipeline in exactly one way: `mav-ffi` (at startup) and `mav-replay`
(per run) register its factory with the engine under its id, and the engine resolves a manifest's
`codec` field against that set. A manifest naming an id nothing registered refuses to install
with `DECODE_CODEC_UNAVAILABLE`. The first such crate is `mav-connector-whoop`, which carries the
WHOOP record decoders, event vocabulary, historical-control layouts, and command builders, and
delegates every layout-DSL packet to the core's `ManifestCodec`.

Its shape, at sketch level:

```rust
trait DeviceCodec {
    fn decode(&mut self, frame: &Frame, manifest: &Manifest, kv: &mut DeviceKv)
        -> Result<RawSampleBatch, MavError>;
}
```

A codec receives three things and nothing else: the bytes of a frame, its own manifest, and a
per-device key-value store scoped to that one device. It returns frames or samples, or an error. The
key-value store is where a learned anchor is read and written, and it is per-device, so one strap's
learned values never leak into another's.

## The boxing rules

The codec is boxed in by its interface, and the boundary is the point of the whole design. A codec:

- **may** read the frame bytes it is given,
- **may** read its own manifest,
- **may** read and write its own per-device key-value store,
- **may not** touch storage directly,
- **may not** touch the network,
- **may not** touch analytics, features, or metrics,
- **may not** see or affect any other device.

A codec cannot reach the parts of the system where a decode bug would become a storage corruption or
a cross-device contamination. The worst a broken codec can do is produce wrong samples for its own
device, and wrong samples are caught by signal quality, plausibility gates, and golden fixtures. It
cannot write directly to a table, cannot phone home, and cannot reach into another strap's state.
That containment is what lets a new codec be written by an agent working a single packet without
putting the rest of the system at risk.

## Adding a device

The whole procedure for a new device is:

1. Write `<device>/manifest.json` in the connectors repository with the static facts above.
2. If, and only if, the device needs logic the DSL cannot express, add a
   `core/connectors/mav-connector-<device>` crate implementing `DeviceCodec`, name it in the
   manifest's `codec` field, and add its `register_codec` line in `mav-ffi` and `mav-replay`.
3. Install the manifest through the runtime (`install_connector`), which validates it — including
   every decoder id it names against what its codec admits.

There is no step that edits a core crate. ADR-012 came from challenging this promise with an
adversarial frame description: it exposed that framing was still a closed WHOOP enum, so framing
became manifest data. ADR-016 came from the promise actually failing — WHOOP decoders had
accreted inside `mav-codec` because compiled device logic had no home — and is why the codec
crates and the registration seam exist. The probe remains as focused unit tests, not as a fake
device connector.

## Where connectors live

Device *manifests* are not part of this repository and are not bundled in the app. They live in
their own repository, `sennnen/maverick-connectors`, and are imported rather than built in, for the
reasons in [ADR-011](adr/ADR-011.md). The app reads connector manifests from that repository or a
local copy of it; the core does not depend on it. The dependency runs one way only: a connector is
validated against the `mav-codec` schema in this repository, and `mav-codec` never learns about any
specific device, which is the boxed-in boundary from [ADR-007](adr/ADR-007.md) expressed as a
repository split.

Device *codecs* are the amendment [ADR-016](adr/ADR-016.md) makes: compiled code cannot be
imported at runtime on a phone, so the codec crates live in this repository under
`core/connectors/`, outside the core crates and boxed behind the trait, linked only by the two
edge crates. A manifest update still ships on the connectors repository's own cadence; a codec
change is an app release, which is what compiled code costs everywhere.

Device manifests needed by core tests are constructed inline rather than pulled from the connectors
repository, so the core stays self-contained. Developing a real vertical slice against a WHOOP
capture needs the connectors repository checked out alongside this one.

The single connector the app itself may carry is a generic Bluetooth heart-rate connector for the
standard GATT profile (`0x180D` / `0x2A37`). That profile is an open standard, not a device family,
so a zero-configuration fallback for it can live in the app without making the app a home for
device-specific code. Everything that decodes a proprietary format is a connector and belongs in the
connectors repository.
