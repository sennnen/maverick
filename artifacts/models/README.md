# Model artefacts

`manifest.json` is the admitted model set. It is the single source of truth for what ships, what
shape each model's tensors are, and which artefact hash each platform must have loaded — and it is
generated, by [tools/ml/build_manifest.py](../../tools/ml/build_manifest.py).

Four files are rendered from it and must never be hand-edited:

- `core/crates/mav-analytic/src/model_zoo/registry.rs`
- `apps/ios/Maverick/Model/MavModelCatalog.swift`
- `apps/android/app/src/main/java/com/sennnen/mav/ml/MavModelCatalog.kt`
- the parity table in `docs/ml.md`

`contracts/` holds the full conversion record for every model that was *attempted*, including the
ones that did not ship: the source archive and its hash, the submodule that was converted, the
parameter count, the parity against PyTorch on each platform, the agreement between the two, and
the converter error where there was one. The manifest lists only the models that passed every gate;
the contracts directory is where the rest of the story is.

## Reading the manifest

| Field | Means |
|---|---|
| `models[].model` | The slug. Also the artefact base name on both platforms and the key in every generated registry. |
| `models[].standing` | `first_party` for Maverick's own weights, `open_licensed` for third-party weights under a permissive licence. Provenance, not quality. |
| `models[].licence` | The notice that has to travel with the artefact, where one does. |
| `models[].coreml.sha256` | Hash of `Data/com.apple.CoreML/model.mlmodel` inside the shipped `.mlpackage` — what the Swift catalogue declares and `tools/check_model_assets.py` proves. |
| `models[].coreml.members` | Every file in the package with its hash, so the gate checks the weights too, not just the graph. |
| `models[].tflite.sha256` | Hash of the shipped flatbuffer. The Android runner recomputes this at load and refuses a mismatch. |
| `models[].*.precision` | Both platforms ship float16 weights with float32 activations. |
| `models[].*.parity` | Worst absolute and relative deviation from the float32 PyTorch reference on one probe input. |
| `models[].cross_platform` | How far the two shipped artefacts disagree with each other on identical tensors. The gate that catches a converter which quietly built a different graph. |
| `not_shipped` | Models that failed a gate, with the reason. Present so a later attempt starts from the failure. |
| `bundle_bytes` | What the admitted set costs each app. |

## The artefacts themselves

They are not in this directory. They live where each build system expects them:

- `apps/ios/Maverick/Models/<slug>.mlpackage`
- `apps/android/app/src/main/assets/models/<slug>.tflite`

`tools/check_model_assets.py` asserts that those two directories contain exactly the manifest's
models plus the ECG classifier, and that every one hashes to its admitted value. It is an equality,
not a subset: an extra model in a bundle is a model nothing validated.

## Standing

Nothing here is clinically validated. Every model is provisional; the contracts, limitations and
validation ceiling are in [docs/ml.md](../../docs/ml.md), and the redistribution position is in
[apps/MODEL-NOTICE.md](../../apps/MODEL-NOTICE.md).
