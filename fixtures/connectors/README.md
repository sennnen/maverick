# Connector parity fixtures

These are signed development artifacts generated from `maverick-connectors` at WC-P11 and their
exact `mavconn-parity/v1` reports. They are product-runtime inputs, not bundled production
connectors: tests install bytes through the public API, and WC-P12 proves no device implementation
is linked into Maverick.

- `generic_hr_v1.mavconn` (1.0.0): SHA-256
  `17f1ee6eee7eea6cd2a03fbcb8c9eada80ae0f4a5a39cab708a949ff5251041a`.
- `whoop4_v1.mavconn` (1.0.3): SHA-256
  `d3dae33eb0849f6eec489473d5ddd38ff39506e74ec40c6ca57a2b513491a145`.
- `whoop5_v1.mavconn` (1.0.7): SHA-256
  `a37e0acdaf161ad1a94fd81d65be9c0572285124a3ee17e262b1bf492b86a7b5`.

Each of those is an `artifact_sha256` the signed registry names at the commit `CONNECTORS_REF`
pins, and `ConnectorParityTest` recomputes them from the bytes on disk rather than trusting this
list. They last moved when ECG capture landed — whoop4 1.0.2 to 1.0.3, whoop5 1.0.5 to 1.0.7 —
and the pin, this list and the frozen expectations in that test all have to move with them, in
the same commit. Missing that is what left the three disagreeing.
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
