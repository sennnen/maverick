# ECG native report test artifacts

These PDFs are synthetic software-regression artifacts, not clinical simulations or diagnostic
evidence.

- `coreml/`: rendered by the native iOS PDF implementation after real Core ML inference.
- `tflite/`: rendered by the native Android PDF implementation after real FP16 TFLite inference
  on a physical Pixel 7.
- Three fixtures exercise sinus rhythm (`N`), three exercise atrial-fibrillation-like irregularity
  (`A`), and three exercise the model's other-abnormal-rhythm bucket (`O`: tachycardia,
  bradycardia, and bigeminy-shaped synthetic signals).

Each report is one A4 page: a text-first result, probabilities, six-segment occlusion explanation,
safety/provenance, and the complete 30-second trace. Each trace is six five-second strips on a true
25 mm/s time base with one shared vertical gain. Millivolt fixtures use 10 mm/mV; sources such as
WHOOP that expose uncalibrated ADC counts use an explicitly labelled shared relative gain rather
than an invented voltage scale. The fixture source is `fixtures/ecg/corpus`.

The matrix is intentionally broader than one happy path per class:

| Model class | Fixtures per runtime |
|---|---|
| `N` — sinus rhythm | regular synthetic rhythms at 55, 72, and 90 bpm |
| `A` — atrial fibrillation | irregular synthetic rhythms at 70, 90, and 110 bpm |
| `O` — other abnormal rhythm | tachycardia at 120 bpm, bradycardia at 40 bpm, and a bigeminy-shaped rhythm at 80 bpm |

Run `tools/check_ecg_reports.sh` from any directory to verify all 18 checksums, page counts, required
text, forbidden branding, and all 18 Poppler-rendered pages. `SHA256SUMS` freezes the exact native
outputs that were visually inspected.
