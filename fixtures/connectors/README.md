# Connector parity fixtures

These are signed development artifacts generated from `maverick-connectors` at WC-P11 and their
exact `mavconn-parity/v1` reports. They are product-runtime inputs, not bundled production
connectors: tests install bytes through the public API, and WC-P12 proves no device implementation
is linked into Maverick.

- `whoop4_v1.mavconn`: SHA-256
  `3158072c210ff18a510e044192a28b781669a276cab6279ed0ae58dfef23c72d`.
- `whoop5_v1.mavconn`: SHA-256
  `3c4c013f6c593c411fb822e65b8c363a6524dbf759390c10781a8bae695cfd47`.
- `*_parity_v1.expected.json`: canonical event, action, sample, final-state, fuel, and linear-memory
  results produced by `mavconn-test --report` from those exact bytes.

Provenance is `[PROV]`: connector-native execution generated the expected action/state cases, and
the no-JIT Wasm runtime must reproduce them byte-for-byte. This proves target consistency, not
physiological or hardware validity. Real record inputs retain their source tags in
`docs/protocol/whoop.md`; synthetic deep buffers remain provisional.

Regenerate only through the connector-authoring and golden-fixtures workflows. Rebuilding either
artifact requires an external temporary test signer, updating the sibling `package-test.json`,
running its deep validator twice for deterministic output, copying the exact artifact/report here,
and deleting the private key before commit. Never hand-edit a report.
