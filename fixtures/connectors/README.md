# Connector parity fixtures

These are signed development artifacts generated from `maverick-connectors` at WC-P11 and their
exact `mavconn-parity/v1` reports. They are product-runtime inputs, not bundled production
connectors: tests install bytes through the public API, and WC-P12 proves no device implementation
is linked into Maverick.

- `generic_hr_v1.mavconn` (1.0.0): SHA-256
  `17f1ee6eee7eea6cd2a03fbcb8c9eada80ae0f4a5a39cab708a949ff5251041a`.
- `whoop4_v1.mavconn` (1.0.3): SHA-256
  `085369dacb6ae747e9bec0a1f8588e18a0e5539e6ea07a4bc41253d607e47304`.
- `whoop5_v1.mavconn` (1.0.7): SHA-256
  `e2b730f03ce313ea0f7d2e234d0412d775ad7aac8bedc9b20cc3c93a55eb9f7c`.

Each of those is an `artifact_sha256` the signed registry names at the commit `CONNECTORS_REF`
pins, and `ConnectorParityTest` recomputes them from the bytes on disk rather than trusting this
list. They last moved when a refused ECG capture started reporting why instead of returning an
empty batch — both connectors, because the shared protocol crate changed with them. The pin, this
list and the frozen expectations in that test all have to move together, in the same commit;
missing that is what left the three disagreeing once before.
- `*_parity_v1.expected.json`: canonical event, action, sample, final-state, fuel, and linear-memory
  results produced by `mavconn-test --report` from those exact bytes.

Provenance is `[PROV]`: connector-native execution generated the expected action/state cases, and
the no-JIT Wasm runtime must reproduce them byte-for-byte. This proves target consistency, not
physiological or hardware validity. Real record inputs retain their source tags in
`docs/protocol/whoop.md`; synthetic deep buffers remain provisional.

Regenerate them, and rotate `CONNECTORS_REF` to the commit that produced them, only through the
connector-authoring and golden-fixtures workflows. In `maverick-connectors`, one command rebuilds and
re-signs the whole set deterministically and copies the exact artifacts and reports here:

    python3 tools/testsign.py --sdk-path ../maverick/core/crates/mav-connector-sdk \
        --tool-dir ../maverick/core/target/release --maverick-root ../maverick

Its Ed25519 test key is committed on purpose — these are sandbox fixtures under publisher
`maverick-whoop-live-test`, trusted only by debug/test builds — so there is no private key to guard or
lose, and `tools/regenerate.py --check` stays the keyless CI freshness gate. Production signing is a
different, external process (`tools/publish.py`). Never hand-edit a report.
