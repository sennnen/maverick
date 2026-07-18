---
name: connector-authoring
description: >
  How to add a new device through the public mav-connector-sdk as one signed .mavconn: implement a
  deterministic event/action state machine, embed fixtures, compile to WebAssembly, validate, and
  publish without editing Maverick. Load when adding a device or changing connector source/metadata.
---

# Authoring a runtime-loaded connector

Target architecture is ADR-017. Runtime/SDK implementation is tracked by WC-P0 through WC-P16; do
not pretend this workflow is runnable before WC-P3 lands.

Rule: **adding a device never edits Maverick core, FFI, iOS, Android, or Cargo workspace.** Connector
source belongs in `sennnen/maverick-connectors` or an independent repository and depends only on the
released public SDK. A missing ABI capability is an ADR/SDK design issue, not permission to add a
device special case.

Read `docs/connectors.md` for artifact, ABI, trust, lifecycle, state, and security contracts. Then:

1. Create a standalone Rust SDK project for one device-family artifact. Share pure code only where
   evidence proves behaviour is common.
2. Declare deterministic metadata: identity/advertisement rules, services/logical characteristics,
   capabilities, ABI/core ranges, state schema, resource profile, and publisher id.
3. Implement protocol as event in, bounded ordered actions out. UUIDs, framing, commands,
   handshake/auth, ACKs, history, retries, firmware quirks, and learned state stay here. Never call
   BLE/native APIs, filesystem, network, clock, randomness, thread, or process.
4. Write exact native/state-machine tests first. Include malformed input, timeout, cancellation,
   reconnect, restart, state corruption, and persist-before-ack ordering.
5. Add provenance-tagged golden fixtures. Use `skills/golden-fixtures` for Maverick-owned fixtures;
   never hand-edit captured evidence.
6. Compile `wasm32-unknown-unknown`; run native/Wasm parity and production-limit malicious tests.
7. Package deterministic custom sections with `mavconn-pack`, sign through a dedicated external
   Ed25519 publisher signer, then run `mavconn-inspect` and `mavconn-validate`.
8. Prove install, update, downgrade refusal, rollback, revocation, uninstall, replay, and both
   platform paths before publishing exact digest-addressed bytes.

Never reuse Android JKS, Apple distribution, or registry keys for connector signing. Never commit or
print private keys. iOS and Android may trust different publishers; artifact and ABI remain one.
