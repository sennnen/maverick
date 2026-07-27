# Connector parity fixtures

These are signed development artifacts generated from `maverick-connectors` at WC-P11 and their
exact `mavconn-parity/v1` reports. They are product-runtime inputs, not bundled production
connectors: tests install bytes through the public API, and WC-P12 proves no device implementation
is linked into Maverick.

- `generic_hr_v1.mavconn`: SHA-256
  `33a7c5141295ef9d5c030cb0706963291c2ce432f9c82a1375367056730c26f2`.
- `whoop4_v1.mavconn`: SHA-256
  `cbba766ea7c9e69b1025274cd56438fa53b78426469ab40b4d30b702af1055d8`.
- `whoop5_v1.mavconn`: SHA-256
  `4046f48d48c943b9e81f7db124a177922507d17dd00f6c76ddffa936cd8f58b4`.
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
