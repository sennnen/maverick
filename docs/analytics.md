# Analytics

Maverick ships an analytic only when the number has an external meaning and the implementation has
evidence beyond agreeing with itself. The first admitted analytic is time-domain variability over
beat-to-beat intervals: mean interval, RMSSD, SDNN, NN50, and pNN50.

## HRV when the source is not an ECG

The 1996 ESC/NASPE Task Force defines RMSSD, SDNN, NN50, and pNN50 over normal-to-normal intervals
measured from cardiac beats. Its definitions are the reference for Maverick's formulas:
[Heart rate variability: standards of measurement, physiological interpretation and clinical use](https://www.escardio.org/static-file/Escardio/Guidelines/Scientific-Statements/guidelines-Heart-Rate-Variability-FT-1996.pdf).

WHOOP's RR stream is derived from optical pulse timing, not ECG R peaks. Its connectors therefore
declare `interval_source: ppg`. That distinction matters.
Optical pulse-rate variability can be useful, but it is not diagnostic ECG HRV and the strap does
not expose a trustworthy normal-beat classifier. `mav-analytic` therefore requires an
`IntervalSource` and labels a PPG result `pulse_rate_variability`. Only an ECG-derived interval
series may be labelled `heart_rate_variability`.

## The admitted calculation

`time_domain` accepts scored `RrInterval` samples, orders them by `(device_time, seq)`, and computes:

- mean interval: arithmetic mean of accepted intervals;
- RMSSD: square root of the mean squared difference between adjacent intervals;
- SDNN: sample standard deviation of accepted intervals;
- NN50: adjacent pairs whose absolute difference is greater than 50 ms;
- pNN50: NN50 divided by the number of adjacent pairs, expressed as a percentage.

The `seq` tiebreaker is load-bearing. Two equal RR intervals in one second are two beats; collapsing
them removes a zero difference and biases RMSSD upward.

The calculation admits intervals only when their upstream quality score is at least 0.5 and their
value is within 300–2000 ms. Excluded intervals are counted. They are not corrected, interpolated,
or replaced. The more sophisticated Lipponen–Tarvainen artefact classifier is real and well
validated, but it is not approximated here: its published method is a distinct algorithm that
deserves its own implementation and validation packet
([paper and validation summary](https://pubmed.ncbi.nlm.nih.gov/31314618/)).

Three accepted intervals are the minimum. This is enough to exercise the formulas, not enough to
claim a clinically meaningful window. Window-length policy belongs to the feature or metric that
uses the calculation; it must not be smuggled into the primitive formula.

## Recovery is not admitted

There is no public WHOOP recovery formula and Maverick has no labelled fixture that maps its own
features to a validated recovery score. A made-up weighted sum would be plausible-looking fiction.
Recovery therefore exists in the capability graph as `algorithm_not_admitted`: even when RR is
present, the core reports Recovery unavailable.

That status changes only when one of these exists:

- a published recovery definition precise enough to reproduce;
- a labelled dataset and independently justified model;
- a compatibility target backed by real input/output fixtures, clearly labelled as compatibility
  rather than physiological truth.

## Capability negotiation

Analytics declare required streams and admission state as data in `mav-analytic`. The UI receives
an availability result; it does not hardcode device-family checks.

- Time-domain interval variability requires `RrInterval` and is admitted.
- Recovery requires `RrInterval` but is not admitted.
- With no RR stream, both report `missing_streams: [rr_interval]`.
- With RR present, variability is available and Recovery reports `algorithm_not_admitted`.

This is a deliberate refusal to fill missing science with product theatre.

## The ported algorithm library (WHOOP-P6)

`mav-analytic` also carries a library of brand-neutral physiological algorithms imported from
`tanarchytan/whoop-rs` (`[WRS]` in the protocol ledger): readiness (log-domain rolling-baseline
RMSSD), resting HR (sustained-floor), recovery (z-score + logistic composite), strain (Karvonen
%HRR TRIMP), stress (Baevsky index), HR zones (Tanaka + time-in-zone), VO2max / fitness age (Nes
HUNT), respiratory rate (RSA), PPG-derived HR (autocorrelation), SpO2 (ratio-of-ratios), IMU
activity features, an at-rest HR watch, and the shared `stats` and `calibration` primitives. Each
is a pure function — plain values in, a wellness estimate or `None` out, no wire types and no IO —
and each is pinned by the upstream's property and recovered-value tests, so it clears the ADR-009
bar of a genuinely-failable test even before a real capture exists.

These modules are a **reviewed library, not yet admitted analytics**. None is wired into the live
snapshot or the capability graph. Promoting one to an emitted, capability-gated analytic is a
separate packet per metric, and it stays gated on the same admission rule: a published reference
reproduced, or a real input/output fixture. `recovery` and `strain` carry an additional
compatibility-estimate label — their denominators and driver weights are the vendor's constants,
refittable from reference pairs, not physiological ground truth — and would be surfaced as
compatibility readouts, never as validated science. The refusal above still stands: shipping a
number into the snapshot is what the admission rule governs, and copying a formula does not by
itself admit it.
