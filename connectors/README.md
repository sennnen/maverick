# Connectors

Device connectors do not live here. They live in their own repository,
[sennnen/maverick-connectors](https://github.com/sennnen/maverick-connectors), because the app
imports connectors rather than bundling them and the core must not depend on them. The reasoning is
recorded in [ADR-011](../docs/adr/ADR-011.md), and the contract a connector is written against is in
[connectors.md](../docs/connectors.md).

No device connector stays in this directory. Milestone 2 briefly used an adversarial frame
description to expose a closed framing assumption; the reusable `FrameSpec` tests remain in the
core, but the fake connector artefacts were removed. Maverick spends connector work on real
hardware.

To develop or test against the real device manifests, clone the connectors repository alongside this
one:

    git clone https://github.com/sennnen/maverick-connectors.git

The one connector the app itself may carry is a generic Bluetooth heart-rate connector for the
standard GATT profile (`0x180D` / `0x2A37`), which is an open standard rather than a device family.
Everything that decodes a proprietary format is a connector and belongs in the connectors repo.
