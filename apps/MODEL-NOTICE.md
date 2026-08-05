# Model notice

The apps bundle fourteen models: the provisional `nao_full_v2` ECG classifier, and the thirteen of
the model zoo.

## Provenance

Thirteen of the fourteen are **Maverick's own**, trained in-house. They ship with the app under no
external restriction, and no third-party permission is needed to redistribute them.

One is not. `pulse_ppg` is Pulse-PPG (Xu et al., *An Open-Source Field-Trained PPG Foundation Model
for Wearable Applications Across Lab and Field Settings*, UbiComp 2025), MIT-licensed, weights
published by its authors. It is redistributable under that licence, and the attribution above
travels with it. The core carries this rather than leaving it to prose: `pulse_ppg` is
`open_licensed` in the registry and `requires_attribution` is true on its FFI descriptor.
Everything else is `first_party`.

## What is and is not established

Every model has a written tensor contract, fixture-covered software behaviour, and a recorded
conversion parity against its PyTorch reference on both platforms. That establishes that the
artefact in the bundle computes what the trained weights compute.

It establishes nothing about sensitivity, specificity, predictive value, or fitness for any
clinical purpose. Every model is **provisional**, and Maverick does not present any of them as a
medical device or a diagnosis.

No model-zoo output reaches a snapshot or a screen. Admitting one is a separate decision per model,
under the rule in `docs/testing.md`.

## Where the details are

Contracts, inputs, outputs, precision, parity and the models that do not ship are in
[`docs/ml.md`](../docs/ml.md). The decision that put them here is
[`ADR-035`](../docs/adr/ADR-035.md). The admitted artefact hashes are in
[`artifacts/models/manifest.json`](../artifacts/models/manifest.json), enforced by
`tools/check_model_assets.py`.
