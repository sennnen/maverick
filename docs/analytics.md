# Analytics

Maverick ships an analytic only when the number has an external meaning and the implementation has
evidence beyond agreeing with itself. The first admitted analytic is time-domain variability over
beat-to-beat intervals: mean interval, RMSSD, SDNN, NN50, and pNN50.

## HRV when the source is not an ECG

The 1996 ESC/NASPE Task Force defines RMSSD, SDNN, NN50, and pNN50 over normal-to-normal intervals
measured from cardiac beats. Its definitions are the reference for Maverick's formulas:
[Heart rate variability: standards of measurement, physiological interpretation and clinical use](https://www.escardio.org/static-file/Escardio/Guidelines/Scientific-Statements/guidelines-Heart-Rate-Variability-FT-1996.pdf).

WHOOP's beats are timed from an optical pulse, not from ECG R peaks. Optical pulse-rate
variability can be useful, but it is not diagnostic ECG HRV and the strap exposes no trustworthy
normal-beat classifier.

The distinction is carried by the stream kind rather than asserted by a caller (ADR-027).
`StreamKind::RrInterval` means an electrical R peak and is the only kind that may be labelled
`heart_rate_variability`; `StreamKind::PulseInterval` means an optical pulse and labels as
`pulse_rate_variability`. A connector declares which one it produces, and reads it from the device
where it can: the Generic HR Monitor publishes electrical intervals only when the Bluetooth SIG
Body Sensor Location characteristic says the sensor sits on the chest.

Two paths reach `RrInterval` today. A chest strap publishes the intervals directly. A device that
exposes the waveform instead — the WHOOP MG's single-lead ECG at 100 Hz — has its R peaks detected
by `mav-analytic::ecg`, which reproduces the Pan-Tompkins (1985) detector: band-pass, derivative,
squaring, moving-window integration, then the paper's two-threshold decision with its refractory
period, T-wave discrimination and search-back. The one deviation is stated in the module: the
paper's integer filters are designed for 200 Hz, so the band-pass is a zero-phase Butterworth
cascade computed for whatever rate the hardware actually samples at.

## The admitted calculation

`time_domain` accepts scored interval samples of one kind, orders them by `(device_time, seq)`, and
computes:

- mean interval: arithmetic mean of accepted intervals;
- RMSSD: square root of the mean squared difference between adjacent intervals;
- SDNN: sample standard deviation of accepted intervals;
- NN50: adjacent pairs whose absolute difference is greater than 50 ms;
- pNN50: NN50 divided by the number of adjacent pairs, expressed as a percentage;
- SD1 and SD2: the Poincaré descriptors, which Brennan, Palaniswami and Kamen (2001) showed are
  exact functions of the measures above rather than a second estimate;
- DFA α1: the short-term detrended fluctuation exponent (Peng et al. 1995), the one measure here
  that describes the correlation structure of the beats rather than their spread.

Frequency-domain band powers are a separate analytic over the same beats. Beats do not arrive on an
even grid, and the usual workaround is to resample before an FFT — which invents values the heart
never produced, and this pipeline does not interpolate. `mav-analytic::frequency` uses the
Lomb-Scargle periodogram (Lomb 1976; Scargle 1982), a least-squares fit of sinusoids directly to
unevenly sampled data; Laguna, Moody and Mark (1998) showed it estimates HRV spectra more
accurately than resampling for exactly that reason. It is the right transform for this data rather
than a compromise. The spectrum is scaled so its integral equals the variance of the series, which
makes the band powers millisecond-squared; the normalised units and the LF/HF ratio are free of
that convention and are what a display should prefer.

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

- Time-domain and frequency-domain variability require either interval stream, and are admitted.
- Recovery requires either interval stream but is not admitted.
- With no interval stream, all three report `missing_streams: [rr_interval]` — the kind they would
  rather have.
- With intervals present, both variability analytics are available and Recovery reports
  `algorithm_not_admitted`.

This is a deliberate refusal to fill missing science with product theatre.

### Cleaning before variability

Every variability measure is built on one shared core, `mav-analytic::intervals`. There used to be
three implementations of RMSSD with three different artefact policies; there is now one, and
everything else calls it.

It applies the range band and then the local-median filter of Karlsson et al. (2012), which rejects
an interval differing from the median of its neighbourhood by more than 20%. It is a local test on
purpose: a resting series drifting slowly is not artefact, while a doubled or dropped beat lands far
from its neighbours however slowly the series is drifting. Every rejected beat is counted in
`excluded_count`. Both halves are needed — a missed or doubled beat lands inside 300–2000 ms and
still produces one enormous successive difference, and RMSSD is the root mean square of those.

A rejected beat is marked, never replaced, and the two differences that touched it disappear with
it. Deleting a beat and then differencing its neighbours manufactures a change spanning two real
beats, which is the same mistake as interpolating and was what the earlier filter did.

Differences never cross a gap in the recording either. A strap delivers beats in bursts minutes
apart, and the difference between the last beat of one burst and the first of the next is not a
beat-to-beat change; against the first live capture that mistake inflated RMSSD roughly tenfold.
Runs are split on the gap and their differences pooled, so every burst in a day contributes — the
earlier design took only the longest run and discarded the rest. `docs/protocol/whoop.md` records
the numbers.

### The DailySnapshot

Analytics reach the apps as one record per `LocalDay`, frozen in [ADR-024](adr/ADR-024.md): the HRV
time-domain values, a readiness tier, recovery and strain each carrying their admission status, a
sleep summary where a night exists, and an **availability list** — one entry per analytic, with an
`UnavailableReason` for everything not served.

The availability list is part of the contract, not a convenience. A card backed by an unavailable
analytic renders the core's reason for it. It never renders a platform-computed substitute, which is
the same rule as the honesty rules in [platform.md](platform.md) and the reason the Android Kotlin
scorers are being deleted rather than kept beside the core: two implementations of a metric are two
answers, and the second one is unaccountable.

Timezone offsets come from the platforms as explicit spans over FFI, into the existing
`Timezone::new(id, spans)`. Rust takes no tzdata dependency: the phone already has a correct and
updated zone database, and it is the only place the user's zone is genuinely known.

### Scorer disposition

The Android app carried four on-device scorers. Their fate, decided in the AS-P4 audit:

| Scorer | Disposition | Why |
|---|---|---|
| `Hrv.rmssd` | **deleted** | `mav-analytic::hrv::time_domain` is the same formula with fixtures. |
| `Zones` / `hrMaxTanaka` | **deleted, replaced by FFI** | `mav-analytic::hr_zones` already held the Tanaka ceiling and the same %HRmax ladder. Kotlin now calls it. |
| `RestScorer` | **unavailable** (`SleepPerformance`) | A sleep composite needs staged sleep. No admitted analytic produces one yet; M4 is where that happens. |
| `IllnessSignalEngine` | **unavailable** (`IllnessRisk`) | Needs multi-day baselines of resting HR, skin temperature, respiration, and variability. The spine builds none of them yet. |
| `CyclePhaseEngine` | **unavailable** (`CyclePhase`) | Needs a nightly skin-temperature series over a cycle. Same gap, longer window. |
| `IllnessWatch` | **deleted** | An earlier, blunter version of the same idea, superseded by the `IllnessRisk` capability. |
| `RouteMath` | **retained** | Route polyline decoding for the workout map. Not a scorer. |

The three unavailable entries are declared in `mav-analytic::capability` with the streams they need,
so the apps render the core's reason rather than a blank card or a locally computed number.

None of the deleted scorers was reachable: `AppViewModel.days()` returned an empty list and the
signals flow was never published, so every one of them was already computing over nothing. Deleting
them removed no working feature — it removed the possibility of a second answer appearing later.

## What changed in the ported library, and why

Several imported modules were reviewed against their own stated references and corrected:

| Module | What it claimed | What it does now |
|---|---|---|
| `spo2` | A 30-night median soft-anchored onto 96.5% | The ratio of ratios, with an explicitly `uncalibrated_percent` beside it, and a change against the wearer's own baseline instead of a manufactured level. Refuses a channel sampled below 10 Hz, where a pulse is not resolvable and the "AC amplitude" is aliasing. |
| `strain` | Per-sample duration from the first two timestamps | The median gap across the series. That number multiplies the whole TRIMP sum, so reading it off two samples scored a whole session at whatever rate it happened to open at. |
| `stress` | A Baevsky histogram anchored to the series minimum | Baevsky's absolute 50 ms grid. Anchoring to the minimum made the modal bin, and so the index, depend on the single shortest beat in the window. |
| `respiratory_rate` | A time axis rebuilt from the cumulative interval sum | The recorded beat times, split into runs on a real gap. The reconstruction silently deleted every dropout and then interpolated a breathing waveform across the hole. |
| `recovery` | A raw z-score of RMSSD against a Gaussian spread | A log-domain z-score for the variability term, so halving and doubling are equal and opposite — and the same domain `readiness` already worked in. |
| `readiness` | A minimum night count taken from the vendor's unlock schedule | Its own statistical minimum: a seven-night baseline needs seven nights. `calibration` holds display schedules and is no longer read as an estimator gate. |
| `vo2max` / `strain` | Two different string parsers for the same profile field | One `BiologicalSex` type. `"F"` scored as a woman in one module and a man in the other. |
| `vo2max` | A strain-to-activity-index mapping with no reference | Deleted. The published HUNT index takes measured weekly aggregates, and inventing a second input to a fitted regression is not applying it. |

## The ported algorithm library (WHOOP-P6/P8)

`mav-analytic` also carries a library of brand-neutral physiological algorithms imported from
`tanarchytan/whoop-rs` (`[WRS]` in the protocol ledger): readiness (log-domain rolling-baseline
RMSSD), resting HR (sustained-floor), recovery (z-score + logistic composite), strain (Karvonen
%HRR TRIMP), stress (Baevsky index), HR zones (Tanaka + time-in-zone), VO2max / fitness age (Nes
HUNT), respiratory rate (RSA), PPG-derived HR (autocorrelation), SpO2 (ratio-of-ratios), IMU
activity features, an at-rest HR watch, and the shared `stats` and `calibration` primitives. The
`sleep` module (WHOOP-P8) adds the two per-30 s-epoch hypnogram stagers — V2 (cardiorespiratory:
z-scored HR/HRV/motion emissions, an HR-flatness deep gate, an R-R RSA respiration term, and
Viterbi smoothing) and V1 (Cole-Kripke actigraphy) — over the same protocol-free inputs. Each is a
pure function — plain values in, a wellness estimate or `None` out, no wire types and no IO — and
each is pinned by the upstream's property and recovered-value tests (the sleep stagers by a
frozen-golden hypnogram that reproduces the whole V2 recipe stage-for-stage), so it clears the
ADR-009 bar of a genuinely-failable test even before a real capture exists.

These modules are a **reviewed library, not yet admitted analytics**. None is wired into the live
snapshot or the capability graph. Promoting one to an emitted, capability-gated analytic is a
separate packet per metric, and it stays gated on the same admission rule: a published reference
reproduced, or a real input/output fixture. `recovery` and `strain` carry an additional
compatibility-estimate label — their denominators and driver weights are the vendor's constants,
refittable from reference pairs, not physiological ground truth — and would be surfaced as
compatibility readouts, never as validated science. The refusal above still stands: shipping a
number into the snapshot is what the admission rule governs, and copying a formula does not by
itself admit it.
