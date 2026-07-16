# Connectors

Device connectors do not live here. They live in their own repository,
[sennnen/maverick-connectors](https://github.com/sennnen/maverick-connectors), because the app
imports connectors rather than bundling them and the core must not depend on them. The reasoning is
recorded in [ADR-011](../docs/adr/ADR-011.md), and the contract a connector is written against is in
[connectors.md](../docs/connectors.md).

What stays in this directory is the mock connector (added in Milestone 2), a deliberately awkward
fake device whose only job is to prove the manifest-plus-codec abstraction survives a second device
with no edits to the core. The mock is not a real device, so it is a core test fixture rather than a
distributable connector, which is why it lives with the core and the WHOOP manifests do not.

To develop or test against the real device manifests, clone the connectors repository alongside this
one:

    git clone https://github.com/sennnen/maverick-connectors.git

The one connector the app itself may carry is a generic Bluetooth heart-rate connector for the
standard GATT profile (`0x180D` / `0x2A37`), which is an open standard rather than a device family.
Everything that decodes a proprietary format is a connector and belongs in the connectors repo.
