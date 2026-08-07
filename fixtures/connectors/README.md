# Connector parity fixtures

These are signed development artifacts generated from `maverick-connectors` at WC-P11 and their
exact `mavconn-parity/v1` reports. They are product-runtime inputs, not bundled production
connectors: tests install bytes through the public API, and WC-P12 proves no device implementation
is linked into Maverick.

- `generic_hr_v1.mavconn` (1.0.0): SHA-256
  `9ac7a6648d2a508998a05797d3c38acd8bb1d28d1322d6352fce989553862d98`.
- `whoop4_v1.mavconn` (1.0.3): SHA-256
  `c7539ff1fdae3a0cdc07aef88bae1ae220345878391e7367973fa0502ecac551`.
- `whoop5_v1.mavconn` (1.0.7): SHA-256
  `6137a0a2e1708f681a4f85d4109f186720ef74114fc2b7f08d3ee30fc19cd427`.

Each of those is an `artifact_sha256` the signed registry names at the commit `CONNECTORS_REF`
pins, and `ConnectorParityTest` — on both platforms — recomputes them from the bytes on disk rather
than trusting this list. All three last moved together when maverick-connectors started staging the
SDK inside its own workspace: the artifacts had been carrying the absolute path of whichever
maverick checkout the SDK was patched in from, so a digest signed on one machine did not rebuild on
another. `whoop5` also moved for the SIG heart-rate fix in the same release. The pin, this list and
the frozen expectations in both tests all have to move together, in the same commit; missing that
is what left the three disagreeing once before.
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
