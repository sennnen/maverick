//! Host-owned ECG calibration, exact recording boundary, native inference handoff and interpretation.

use mav_analytic::ecg_model::{inference_tensors, prepare_ecg, EcgUnit};
use mav_analytic::ecg_quality::{assess_ecg_quality, EcgQualityReason};
use mav_model::ecg::{EcgExplanationSegment, EcgInferenceEvidence, EcgResult, EcgRhythmClass};
use mav_model::error::{codes, MavError, Result};
use mav_model::{DeviceId, EcgCaptureId};
use sha2::{Digest, Sha256};

pub const ECG_RECORDING_SECONDS: u32 = 30;
pub const ECG_CALIBRATION_GOOD_SECONDS: u32 = 3;
pub const ECG_CALIBRATION_TIMEOUT_MS: i64 = 30_000;
const ECG_QUALITY_WINDOW_SECONDS: usize = 5;
pub const ECG_PREPROCESSING_SHA256: &str =
    "793dddb8f59e71d8a9b24cbd03e02efe0b361879027cf525a2a3dd6435edff24";
pub const ECG_COREML_SHA256: &str =
    "24230f1c635ab45fba02bb80c86f3b9a9dc2499436bda16a1d97b34ffb63f6d3";
pub const ECG_TFLITE_SHA256: &str =
    "0be97329077d5d5d2791b8b7850baf8acaf8f12f96fdfad7bcdb4af37156ea21";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcgCapturePhase {
    Calibrating,
    Recording,
    Analysing,
    Result,
    Failed,
    Cancelled,
}

impl EcgCapturePhase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Calibrating => "calibrating",
            Self::Recording => "recording",
            Self::Analysing => "analysing",
            Self::Result => "result",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcgCaptureSnapshot {
    pub capture_id: EcgCaptureId,
    pub phase: EcgCapturePhase,
    pub progress_milli: u16,
    pub quality_milli: u16,
    pub quality_reason: Option<String>,
    pub recorded_samples: u32,
    pub target_samples: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EcgInferenceRequest {
    pub capture_id: EcgCaptureId,
    /// Baseline first, followed by six ordered five-second occlusions.
    pub tensors: Vec<Vec<f32>>,
}

pub struct EcgCaptureController {
    capture_id: EcgCaptureId,
    device_id: DeviceId,
    source_rate_hz: u32,
    source_unit: String,
    capture_started_ms: i64,
    recording_started_ns: Option<i64>,
    phase: EcgCapturePhase,
    calibration: Vec<f64>,
    consecutive_good_samples: usize,
    quality_milli: u16,
    quality_reason: Option<String>,
    recording: Vec<f32>,
    request: Option<EcgInferenceRequest>,
    raw_sha256: Option<String>,
    tensor_sha256: Option<String>,
    result: Option<EcgResult>,
}

impl EcgCaptureController {
    pub fn begin(
        capture_id: EcgCaptureId,
        device_id: DeviceId,
        source_rate_hz: u32,
        source_unit: impl Into<String>,
        now_ms: i64,
    ) -> Result<Self> {
        if capture_id.get() == 0 || device_id.get() == 0 || source_rate_hz == 0 {
            return Err(state_error(
                "ECG capture requires positive ids and sample rate",
            ));
        }
        Ok(Self {
            capture_id,
            device_id,
            source_rate_hz,
            source_unit: source_unit.into(),
            capture_started_ms: now_ms,
            recording_started_ns: None,
            phase: EcgCapturePhase::Calibrating,
            calibration: Vec::new(),
            consecutive_good_samples: 0,
            quality_milli: 0,
            quality_reason: Some("contact".to_owned()),
            recording: Vec::new(),
            request: None,
            raw_sha256: None,
            tensor_sha256: None,
            result: None,
        })
    }

    /// Returns true when the host must stop the connector stream.
    pub fn ingest(&mut self, samples: &[f64], now_ms: i64) -> Result<bool> {
        if samples.iter().any(|sample| !sample.is_finite()) {
            self.fail("dropout");
            return Ok(true);
        }
        match self.phase {
            EcgCapturePhase::Calibrating => self.ingest_calibration(samples, now_ms),
            EcgCapturePhase::Recording => self.ingest_recording(samples),
            _ => Err(state_error(
                "ECG samples arrived outside calibration or recording",
            )),
        }
    }

    /// Expire a calibration that ran out of time, on the clock rather than on arriving samples.
    ///
    /// [`Self::ingest`] can only judge the deadline when a sample turns up, so a stream that never
    /// starts — a strap that went off-wrist, a raw stream the firmware silently refused — left the
    /// capture calibrating for as long as the screen stayed open, with the raw stream still
    /// running. Returns true when the host must stop the connector stream.
    pub fn expire_stalled(&mut self, now_ms: i64) -> bool {
        if self.phase != EcgCapturePhase::Calibrating
            || now_ms.saturating_sub(self.capture_started_ms) < ECG_CALIBRATION_TIMEOUT_MS
        {
            return false;
        }
        self.fail(if self.calibration.is_empty() {
            "no_signal"
        } else {
            "calibration_timeout"
        });
        true
    }

    fn ingest_calibration(&mut self, samples: &[f64], now_ms: i64) -> Result<bool> {
        if now_ms.saturating_sub(self.capture_started_ms) >= ECG_CALIBRATION_TIMEOUT_MS {
            self.fail("calibration_timeout");
            return Ok(true);
        }
        self.calibration.extend_from_slice(samples);
        // Abrupt contact loss, rails, dropout, or motion should reset promptly. The short window
        // is deliberately not allowed to decide `Contact`: at slow rates it may legitimately hold
        // fewer than two retained R peaks, which is why positive contact evidence uses the longer
        // window below.
        let short_window = self.source_rate_hz as usize * 2;
        if self.calibration.len() >= short_window {
            let start = self.calibration.len() - short_window;
            let short_quality =
                assess_ecg_quality(&self.calibration[start..], f64::from(self.source_rate_hz));
            if !short_quality.good
                && !matches!(short_quality.reason, Some(EcgQualityReason::Contact))
            {
                self.consecutive_good_samples = 0;
                self.quality_milli = short_quality.score_milli;
                self.quality_reason = short_quality
                    .reason
                    .map(quality_reason_name)
                    .map(str::to_owned);
                return Ok(false);
            }
        }
        // Two seconds is enough for the scalar checks, but not enough for the recovered beat
        // detector to consistently retain two R peaks after its learning and edge windows. A
        // clean 72 bpm lead resampled to WHOOP's 100 Hz otherwise times out as "contact", and
        // slower rhythms are even more exposed. Five seconds remains responsive while giving the
        // rhythm-independent contact check enough evidence.
        let window = self.source_rate_hz as usize * ECG_QUALITY_WINDOW_SECONDS;
        if self.calibration.len() < window {
            return Ok(false);
        }
        let start = self.calibration.len() - window;
        let quality =
            assess_ecg_quality(&self.calibration[start..], f64::from(self.source_rate_hz));
        self.quality_milli = quality.score_milli;
        self.quality_reason = quality.reason.map(quality_reason_name).map(str::to_owned);
        if quality.good {
            self.consecutive_good_samples =
                self.consecutive_good_samples.saturating_add(samples.len());
        } else {
            self.consecutive_good_samples = 0;
        }
        let needed = self.source_rate_hz as usize * ECG_CALIBRATION_GOOD_SECONDS as usize;
        if self.consecutive_good_samples >= needed {
            self.phase = EcgCapturePhase::Recording;
            self.recording_started_ns = Some(now_ms.saturating_mul(1_000_000));
            self.calibration.clear();
            self.quality_reason = None;
        } else if self.calibration.len() > window {
            self.calibration.drain(..self.calibration.len() - window);
        }
        Ok(false)
    }

    fn ingest_recording(&mut self, samples: &[f64]) -> Result<bool> {
        let target = self.target_samples() as usize;
        let remaining = target.saturating_sub(self.recording.len());
        self.recording
            .extend(samples.iter().take(remaining).map(|sample| *sample as f32));
        if self.recording.len() < target {
            return Ok(false);
        }
        let prepared = prepare_ecg(
            &self.recording,
            self.source_rate_hz,
            unit_from_name(&self.source_unit),
        )
        .map_err(|error| {
            MavError::new(codes::ML_ECG_PREPROCESSING, "ECG preprocessing failed")
                .context(format!("{error:?}"))
        })?;
        let tensors = inference_tensors(&prepared.normalized).map_err(|error| {
            MavError::new(
                codes::ML_ECG_PREPROCESSING,
                "ECG XAI tensor creation failed",
            )
            .context(format!("{error:?}"))
        })?;
        self.raw_sha256 = Some(hash_f32(&self.recording));
        self.tensor_sha256 = Some(hash_f32(&prepared.normalized));
        self.request = Some(EcgInferenceRequest {
            capture_id: self.capture_id,
            tensors,
        });
        self.phase = EcgCapturePhase::Analysing;
        Ok(true)
    }

    pub fn inference_request(&self) -> Option<EcgInferenceRequest> {
        self.request.clone()
    }

    pub fn submit_inference(
        &mut self,
        capture_id: EcgCaptureId,
        predictions: Vec<[f32; 3]>,
        model_sha256: String,
        now_ms: i64,
    ) -> Result<(EcgInferenceEvidence, EcgResult)> {
        if self.phase != EcgCapturePhase::Analysing || capture_id != self.capture_id {
            return Err(state_error(
                "ECG inference does not match the active analysis",
            ));
        }
        validate_predictions(&predictions, &model_sha256)?;
        let started_ns = self
            .recording_started_ns
            .ok_or_else(|| state_error("ECG recording has no start time"))?;
        let ended_ns = started_ns.saturating_add(i64::from(ECG_RECORDING_SECONDS) * 1_000_000_000);
        let evidence = EcgInferenceEvidence {
            capture_id,
            device_id: self.device_id,
            started_ns,
            ended_ns,
            source_rate_hz: self.source_rate_hz,
            source_unit: self.source_unit.clone(),
            sample_count: self.target_samples(),
            raw_sha256: self.raw_sha256.clone().unwrap_or_default(),
            tensor_sha256: self.tensor_sha256.clone().unwrap_or_default(),
            preprocessing_sha256: ECG_PREPROCESSING_SHA256.to_owned(),
            model_sha256,
            quality_milli: self.quality_milli,
            predictions,
            created_ns: now_ms.saturating_mul(1_000_000),
        };
        let mut result = interpret_evidence(&evidence)?;
        result.mean_heart_rate_bpm = mean_heart_rate(&self.recording, self.source_rate_hz);
        self.result = Some(result.clone());
        self.request = None;
        self.phase = EcgCapturePhase::Result;
        Ok((evidence, result))
    }

    pub fn snapshot(&self) -> EcgCaptureSnapshot {
        let target = self.target_samples();
        let progress_milli = match self.phase {
            EcgCapturePhase::Calibrating => {
                let needed = self.source_rate_hz as usize * ECG_CALIBRATION_GOOD_SECONDS as usize;
                ((self.consecutive_good_samples.min(needed) * 1_000) / needed.max(1)) as u16
            }
            EcgCapturePhase::Recording => {
                ((self.recording.len().min(target as usize) * 1_000) / target.max(1) as usize)
                    as u16
            }
            EcgCapturePhase::Analysing | EcgCapturePhase::Result => 1_000,
            EcgCapturePhase::Failed | EcgCapturePhase::Cancelled => 0,
        };
        EcgCaptureSnapshot {
            capture_id: self.capture_id,
            phase: self.phase,
            progress_milli,
            quality_milli: self.quality_milli,
            quality_reason: self.quality_reason.clone(),
            recorded_samples: self.recording.len() as u32,
            target_samples: target,
        }
    }

    pub fn cancel(&mut self) {
        if !matches!(
            self.phase,
            EcgCapturePhase::Result | EcgCapturePhase::Failed
        ) {
            self.phase = EcgCapturePhase::Cancelled;
            self.quality_reason = Some("cancelled".to_owned());
            self.request = None;
        }
    }

    pub fn fail(&mut self, reason: &str) {
        self.phase = EcgCapturePhase::Failed;
        self.quality_reason = Some(reason.to_owned());
        self.request = None;
    }

    fn target_samples(&self) -> u32 {
        self.source_rate_hz.saturating_mul(ECG_RECORDING_SECONDS)
    }
}

/// Mean rate over a recording, from the admitted R-peak detector.
///
/// `None` rather than a fabricated number when the detector finds too few beats to average: on a
/// contact-degraded recording that is the honest answer, and it is what the high- and low-rate
/// checks need in order to say nothing rather than something wrong.
pub fn mean_heart_rate(samples: &[f32], sample_rate_hz: u32) -> Option<u16> {
    if sample_rate_hz == 0 || samples.is_empty() {
        return None;
    }
    let signal: Vec<f64> = samples.iter().map(|value| f64::from(*value)).collect();
    let peaks = mav_analytic::ecg::r_peaks(&signal, f64::from(sample_rate_hz));
    if peaks.len() < 3 {
        return None;
    }
    let span = peaks.last()?.checked_sub(*peaks.first()?)?;
    if span == 0 {
        return None;
    }
    let seconds = span as f64 / f64::from(sample_rate_hz);
    let bpm = (peaks.len() - 1) as f64 * 60.0 / seconds;
    if !bpm.is_finite() || !(20.0..=300.0).contains(&bpm) {
        return None;
    }
    Some(bpm.round() as u16)
}

pub fn interpret_evidence(evidence: &EcgInferenceEvidence) -> Result<EcgResult> {
    validate_predictions(&evidence.predictions, &evidence.model_sha256)?;
    let baseline = evidence.predictions[0];
    let winner = baseline
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| index)
        .ok_or_else(|| inference_error("ECG inference returned no classes"))?;
    let rhythm = EcgRhythmClass::from_model_index(winner)
        .ok_or_else(|| inference_error("ECG inference returned an unknown class"))?;
    // The model's own posterior for the class it chose. The previous formula scaled the
    // top-two margin by 0.2 and clamped, which saturated at 100% for any margin above 0.2:
    // a 66/34 split and a 99/1 split both reported certainty. Reporting the winning
    // probability cannot saturate, matches the three figures shown beside it, and claims
    // nothing the model did not output.
    let confidence_milli = (baseline[winner].clamp(0.0, 1.0) * 1_000.0).round() as u16;

    let raw_importance: Vec<f32> = evidence.predictions[1..]
        .iter()
        .map(|prediction| (baseline[winner] - prediction[winner]).max(0.0))
        .collect();
    let maximum = raw_importance.iter().copied().fold(0.0_f32, f32::max);
    let explanation = raw_importance
        .iter()
        .enumerate()
        .map(|(index, importance)| EcgExplanationSegment {
            start_second: (index * 5) as u8,
            end_second: (index * 5 + 5) as u8,
            importance_milli: if maximum > 0.0 {
                ((*importance / maximum) * 1_000.0).round() as u16
            } else {
                0
            },
        })
        .collect();

    Ok(EcgResult {
        capture_id: evidence.capture_id,
        device_id: evidence.device_id,
        started_ns: evidence.started_ns,
        ended_ns: evidence.ended_ns,
        source_rate_hz: evidence.source_rate_hz,
        sample_count: evidence.sample_count,
        rhythm,
        probabilities: baseline,
        confidence_milli,
        quality_milli: evidence.quality_milli,
        // Filled by the controller, which holds the samples; a rebuild from evidence alone has
        // no waveform and honestly reports no rate.
        mean_heart_rate_bpm: None,
        explanation,
        raw_sha256: evidence.raw_sha256.clone(),
        tensor_sha256: evidence.tensor_sha256.clone(),
        preprocessing_sha256: evidence.preprocessing_sha256.clone(),
        model_sha256: evidence.model_sha256.clone(),
        algorithm_id: "nao_full_v2_ecg_classifier".to_owned(),
        algorithm_version: "2.0.0".to_owned(),
        provisional: true,
    })
}

fn validate_predictions(predictions: &[[f32; 3]], model_sha256: &str) -> Result<()> {
    if !matches!(model_sha256, ECG_COREML_SHA256 | ECG_TFLITE_SHA256) {
        return Err(inference_error("ECG model hash is not admitted"));
    }
    if predictions.len() != 7 {
        return Err(inference_error(
            "ECG inference must return one baseline and six occlusions",
        ));
    }
    for values in predictions {
        if values
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            || (values.iter().sum::<f32>() - 1.0).abs() > 0.05
        {
            return Err(inference_error(
                "ECG inference probabilities are not finite and normalized",
            ));
        }
    }
    Ok(())
}

fn hash_f32(values: &[f32]) -> String {
    let mut digest = Sha256::new();
    for value in values {
        digest.update(value.to_le_bytes());
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unit_from_name(name: &str) -> EcgUnit {
    match name {
        "microvolts" => EcgUnit::Microvolts,
        "millivolts" => EcgUnit::Millivolts,
        "volts" => EcgUnit::Volts,
        _ => EcgUnit::Unknown,
    }
}

fn quality_reason_name(reason: EcgQualityReason) -> &'static str {
    match reason {
        EcgQualityReason::Contact => "contact",
        EcgQualityReason::Motion => "motion",
        EcgQualityReason::Saturation => "saturation",
        EcgQualityReason::Flatline => "flatline",
        EcgQualityReason::Dropout => "dropout",
    }
}

fn state_error(message: &str) -> MavError {
    MavError::new(codes::ML_ECG_CAPTURE_STATE, message)
}

fn inference_error(message: &str) -> MavError {
    MavError::new(codes::ML_ECG_INFERENCE_INVALID, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<f64> {
        include_str!("../../../../fixtures/ecg/n_regular_72_v1.csv")
            .lines()
            .skip(1)
            .map(|line| line.split(',').nth(1).unwrap().parse().unwrap())
            .collect()
    }

    fn controller(rate: u32) -> EcgCaptureController {
        EcgCaptureController::begin(
            EcgCaptureId::new(7),
            DeviceId::new(9),
            rate,
            "millivolts",
            1_000,
        )
        .unwrap()
    }

    fn calibrate(controller: &mut EcgCaptureController, signal: &[f64], rate: usize) {
        for second in 0..10 {
            controller
                .ingest(
                    &signal[second * rate..(second + 1) * rate],
                    2_000 + second as i64 * 1_000,
                )
                .unwrap();
            if controller.snapshot().phase == EcgCapturePhase::Recording {
                return;
            }
        }
        panic!("clean signal did not calibrate");
    }

    #[test]
    fn bad_quality_resets_the_continuous_calibration_window() {
        let signal = fixture();
        let mut capture = controller(256);
        for second in 0..10 {
            capture
                .ingest(
                    &signal[second * 256..(second + 1) * 256],
                    2_000 + second as i64 * 1_000,
                )
                .unwrap();
            if capture.snapshot().progress_milli > 0 {
                break;
            }
        }
        assert!(capture.snapshot().progress_milli > 0);
        capture.ingest(&vec![1.0; 512], 12_000).unwrap();
        assert_eq!(capture.snapshot().progress_milli, 0);
        assert_eq!(
            capture.snapshot().quality_reason.as_deref(),
            Some("flatline")
        );
    }

    #[test]
    fn recording_takes_exactly_thirty_seconds_and_ignores_batch_tail() {
        let signal = fixture();
        let mut capture = controller(256);
        calibrate(&mut capture, &signal, 256);
        assert!(!capture.ingest(&signal[..7_679], 8_000).unwrap());
        assert_eq!(capture.snapshot().recorded_samples, 7_679);
        assert!(capture.ingest(&signal[7_679..], 38_000).unwrap());
        assert_eq!(capture.snapshot().recorded_samples, 7_680);
        assert_eq!(capture.snapshot().phase, EcgCapturePhase::Analysing);
        let request = capture.inference_request().unwrap();
        assert_eq!(request.tensors.len(), 7);
        assert!(request.tensors.iter().all(|tensor| tensor.len() == 7_680));
    }

    /// The whole capture on real hardware: a worn WHOOP MG at 100 Hz in raw converter counts, fed
    /// one 100-sample frame per second exactly as the strap sends them.
    #[test]
    fn a_live_mg_capture_calibrates_records_and_reaches_analysis() {
        let signal: Vec<f64> = include_str!("../../../../fixtures/ecg/mg_electrode_100hz_v1.csv")
            .lines()
            .skip(1)
            .map(|line| line.split(',').nth(1).unwrap().parse().unwrap())
            .collect();
        let mut capture =
            EcgCaptureController::begin(EcgCaptureId::new(7), DeviceId::new(1), 100, "counts", 0)
                .unwrap();

        let mut calibrated_after = None;
        let mut stopped_at = None;
        for (second, frame) in signal.chunks(100).enumerate() {
            let stop = capture.ingest(frame, second as i64 * 1_000).unwrap();
            if calibrated_after.is_none() && capture.snapshot().phase == EcgCapturePhase::Recording
            {
                calibrated_after = Some(second);
            }
            if stop {
                stopped_at = Some(second);
                break;
            }
        }

        assert_eq!(calibrated_after, Some(7), "calibration second");
        assert_eq!(stopped_at, Some(37), "capture stops after exactly 30 s");
        let snapshot = capture.snapshot();
        assert_eq!(snapshot.phase, EcgCapturePhase::Analysing);
        assert_eq!(snapshot.recorded_samples, 3_000);
        assert_eq!(snapshot.quality_reason, None);

        let request = capture.inference_request().unwrap();
        assert_eq!(request.tensors.len(), 7);
        for tensor in &request.tensors {
            assert_eq!(tensor.len(), 7_680);
            assert!(tensor.iter().all(|value| value.is_finite()));
        }
        // Each occlusion masks its own five seconds of the 256 Hz tensor and nothing else.
        for tensor in &request.tensors[1..] {
            let differing = tensor
                .iter()
                .zip(&request.tensors[0])
                .filter(|(masked, baseline)| masked != baseline)
                .count();
            assert_eq!(differing, 1_280);
        }
    }

    #[test]
    fn a_capture_whose_stream_never_arrives_expires_instead_of_calibrating_forever() {
        let mut capture = controller(100);
        assert!(!capture.expire_stalled(1_000 + ECG_CALIBRATION_TIMEOUT_MS - 1));
        assert_eq!(capture.snapshot().phase, EcgCapturePhase::Calibrating);

        assert!(capture.expire_stalled(1_000 + ECG_CALIBRATION_TIMEOUT_MS));
        assert_eq!(capture.snapshot().phase, EcgCapturePhase::Failed);
        assert_eq!(
            capture.snapshot().quality_reason.as_deref(),
            Some("no_signal")
        );
        // Already expired, so it does not ask the host to stop the stream a second time.
        assert!(!capture.expire_stalled(1_000 + ECG_CALIBRATION_TIMEOUT_MS * 2));
    }

    #[test]
    fn a_stalled_capture_that_did_receive_samples_names_the_timeout_not_a_missing_stream() {
        let mut capture = controller(100);
        capture.ingest(&[1.0, 2.0, 3.0], 2_000).unwrap();
        assert!(capture.expire_stalled(1_000 + ECG_CALIBRATION_TIMEOUT_MS));
        assert_eq!(
            capture.snapshot().quality_reason.as_deref(),
            Some("calibration_timeout")
        );
    }

    #[test]
    fn timeout_cancel_and_illegal_ingest_are_explicit_states() {
        let mut timeout = controller(100);
        assert!(timeout.ingest(&[1.0, 2.0], 31_000).unwrap());
        assert_eq!(timeout.snapshot().phase, EcgCapturePhase::Failed);
        assert_eq!(
            timeout.snapshot().quality_reason.as_deref(),
            Some("calibration_timeout")
        );

        let mut cancelled = controller(100);
        cancelled.cancel();
        assert_eq!(cancelled.snapshot().phase, EcgCapturePhase::Cancelled);
        assert_eq!(
            cancelled.ingest(&[1.0], 2_000).unwrap_err().code,
            codes::ML_ECG_CAPTURE_STATE
        );
    }

    #[test]
    fn bounded_predictions_produce_stable_class_confidence_and_xai() {
        let signal = fixture();
        let mut capture = controller(256);
        calibrate(&mut capture, &signal, 256);
        assert!(capture.ingest(&signal, 38_000).unwrap());
        let mut predictions = vec![[0.70, 0.10, 0.20]; 7];
        predictions[1] = [0.50, 0.15, 0.35];
        let (evidence, result) = capture
            .submit_inference(
                EcgCaptureId::new(7),
                predictions,
                ECG_COREML_SHA256.to_owned(),
                39_000,
            )
            .unwrap();
        assert_eq!(result.rhythm, EcgRhythmClass::SinusRhythm);
        // The winning posterior itself, not a saturating margin: 0.70 reports as 700.
        assert_eq!(result.confidence_milli, 700);
        assert_eq!(result.explanation.len(), 6);
        assert_eq!(result.explanation[0].importance_milli, 1_000);
        assert_eq!(result.ended_ns - result.started_ns, 30_000_000_000);
        // The controller holds the waveform, so it can state a rate; the 72 bpm reference
        // fixture reads back as such.
        let rate = result
            .mean_heart_rate_bpm
            .expect("controller measured a rate");
        assert!((60..=85).contains(&rate), "unexpected rate {rate}");
        // A rebuild from stored evidence reproduces every interpreted field, except the rate:
        // evidence carries predictions, not samples, so it honestly reports none (ADR-034).
        let rebuilt = interpret_evidence(&evidence).unwrap();
        assert_eq!(rebuilt.mean_heart_rate_bpm, None);
        assert_eq!(
            EcgResult {
                mean_heart_rate_bpm: result.mean_heart_rate_bpm,
                ..rebuilt
            },
            result
        );
    }

    #[test]
    fn malformed_native_output_and_wrong_model_are_rejected() {
        let evidence = EcgInferenceEvidence {
            capture_id: EcgCaptureId::new(1),
            device_id: DeviceId::new(1),
            started_ns: 0,
            ended_ns: 30_000_000_000,
            source_rate_hz: 100,
            source_unit: "counts".to_owned(),
            sample_count: 3_000,
            raw_sha256: "a".repeat(64),
            tensor_sha256: "b".repeat(64),
            preprocessing_sha256: ECG_PREPROCESSING_SHA256.to_owned(),
            model_sha256: "unadmitted".to_owned(),
            quality_milli: 900,
            predictions: vec![[0.7, 0.1, 0.2]; 7],
            created_ns: 40_000_000_000,
        };
        assert_eq!(
            interpret_evidence(&evidence).unwrap_err().code,
            codes::ML_ECG_INFERENCE_INVALID
        );
        let mut malformed = evidence;
        malformed.model_sha256 = ECG_TFLITE_SHA256.to_owned();
        malformed.predictions[0][0] = f32::NAN;
        assert_eq!(
            interpret_evidence(&malformed).unwrap_err().code,
            codes::ML_ECG_INFERENCE_INVALID
        );
    }
}
