# Android shell

The thin native layer for Android: BLE stack ownership, the UniFFI binding to the core, and
eventually the UI that renders snapshots. Deliberately kept as small as possible; anything worth
getting wrong belongs in `core/`. First real work lands with the Milestone 0 binding packet and
the Milestone 1 parity harness.
