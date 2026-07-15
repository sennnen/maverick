# Connectors

One folder per device. Each holds a `manifest.json` describing everything static about the device
(identity, GATT UUIDs, frame parameters, packet map, field layouts, unit conversions, record
versions, sensor configs) and, only where static data genuinely cannot express the behaviour, a
small codec crate. The contract, and the argument for why it is shaped this way, is in
[docs/connectors.md](../docs/connectors.md); the step-by-step guide for adding a device is
[skills/connector-authoring/SKILL.md](../skills/connector-authoring/SKILL.md).

The hard rule: adding a device changes nothing in `core/crates`. If it seems to need to, that is
an interface dispute and goes to an ADR.

Planned first residents: `whoop5/` (Milestone 1), `mock/` (Milestone 2, deliberately awkward, to
prove the abstraction), `whoop4/` (Milestone 5, historical sync). None exist yet; the manifests
are written by their milestone packets, against the facts in
[docs/protocol/whoop.md](../docs/protocol/whoop.md).
