# ECG model development notice

The iOS FP16 Core ML package and Android FP16-weight TFLite file under the app projects are local
development copies of the recovered `nao_full_v2` ECG classifier. They are provisional,
research-only, and not clinically validated. The model owner's permission is required before
redistributing either binary outside a local development build.

Maverick does not present these weights as a medical device or a diagnosis. The admitted hashes,
tensor contract, limitations and validation ceiling are recorded in `docs/ml.md`.
