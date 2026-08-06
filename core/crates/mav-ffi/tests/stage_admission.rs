#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Queueing a stage across the FFI boundary: what the admission says about its own input, and
//! what it refuses to queue twice.
//!
//! Most of this is the cycle heads, because those six models are the ones this build newly
//! reaches and the thing most worth testing about them is not that they queue — it is that a
//! caller cannot fail to notice when the tensors behind them are padding. A wearer whose skin
//! temperature falls outside the band the archive accepts gets a forty-day series of zeros, and
//! the model returns an ovulation probability for it either way.

use mav_ffi::{MavRuntime, ModelTensor, RuntimeConfig};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DB: AtomicU64 = AtomicU64::new(1);

fn runtime() -> (std::sync::Arc<MavRuntime>, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "mav-ffi-cycle-{}-{}.sqlite",
        std::process::id(),
        NEXT_DB.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    let runtime = MavRuntime::new(RuntimeConfig {
        database_path: path.to_string_lossy().into_owned(),
        timezone_id: "Europe/London".to_owned(),
        app_version: "0.1.0".to_owned(),
    })
    .expect("runtime");
    (runtime, path)
}

/// A history of `days` identical, entirely plausible cycle days.
fn plausible(days: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    (vec![36.4; days], vec![15.0; days], vec![58.0; days])
}

#[test]
fn a_full_plausible_history_queues_every_cycle_head_and_reports_sound() {
    let (runtime, path) = runtime();
    let (temperature, breath, heart) = plausible(40);
    let admitted = runtime
        .admit_cycle_stages(
            temperature,
            breath,
            heart,
            Some(31.0),
            Some(29.0),
            Some(13.0),
        )
        .expect("admitted");

    assert_eq!(admitted.len(), 6, "six cycle heads share this input pair");
    for stage in &admitted {
        assert_eq!(stage.applicability, "sound", "{}", stage.model_slug);
        assert!((stage.real_fraction - 1.0).abs() < 1e-6);
        assert!(stage.substitutions.is_empty());
        assert!(
            stage.request_id.is_some(),
            "{} was not queued",
            stage.model_slug
        );
        assert!(!stage.already_known);
    }
    let slugs: Vec<&str> = admitted.iter().map(|s| s.model_slug.as_str()).collect();
    assert!(slugs.contains(&"popsicle_ovulation_detection"));
    assert!(slugs.contains(&"popsicle_period_prediction"));
    assert!(
        !slugs.contains(&"popsicle_min_follicular"),
        "the min-follicular heads take a different tensor and must not be queued here"
    );
    let _ = std::fs::remove_file(path);
}

/// The case the health record exists for.
#[test]
fn a_history_the_archive_rejects_is_queued_and_flagged_unfounded() {
    let (runtime, path) = runtime();
    // Every column outside the band the archive accepts.
    let admitted = runtime
        .admit_cycle_stages(
            vec![33.0; 40],
            vec![40.0; 40],
            vec![190.0; 40],
            Some(31.0),
            Some(29.0),
            Some(13.0),
        )
        .expect("admitted");

    for stage in &admitted {
        assert_eq!(stage.applicability, "unfounded", "{}", stage.model_slug);
        assert_eq!(stage.real_fraction, 0.0);
        assert!(stage.substitutions.contains(&"out_of_range".to_owned()));
        // It is still queued: the model may run and the result may be stored. What must not
        // happen is a surface reading it as a number about this wearer.
        assert!(stage.request_id.is_some());
    }
    let _ = std::fs::remove_file(path);
}

/// A short history is padding-dominated, and says so rather than looking like a full window.
#[test]
fn a_short_history_reports_the_padding_that_fills_the_window() {
    let (runtime, path) = runtime();
    let (temperature, breath, heart) = plausible(4);
    let admitted = runtime
        .admit_cycle_stages(temperature, breath, heart, None, None, None)
        .expect("admitted");
    for stage in &admitted {
        assert_eq!(stage.applicability, "unfounded");
        assert!(
            (stage.real_fraction - 0.1).abs() < 1e-6,
            "{}",
            stage.real_fraction
        );
        assert!(stage.substitutions.contains(&"missing".to_owned()));
    }
    let _ = std::fs::remove_file(path);
}

/// A non-finite value is the FFI's spelling of "not recorded", and must land as missing rather
/// than as a number.
#[test]
fn a_non_finite_reading_is_treated_as_unrecorded() {
    let (runtime, path) = runtime();
    let mut temperature = vec![36.4; 40];
    temperature[0] = f32::NAN;
    let admitted = runtime
        .admit_cycle_stages(
            temperature,
            vec![15.0; 40],
            vec![58.0; 40],
            None,
            None,
            None,
        )
        .expect("admitted");
    let stage = &admitted[0];
    assert!(stage.substitutions.contains(&"missing".to_owned()));
    assert_eq!(
        stage.applicability, "sound",
        "one cell of 120 is still sound"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn columns_of_different_lengths_are_refused_rather_than_zipped_short() {
    let (runtime, path) = runtime();
    let error = runtime
        .admit_cycle_stages(
            vec![36.4; 40],
            vec![15.0; 39],
            vec![58.0; 40],
            None,
            None,
            None,
        )
        .expect_err("mismatched columns");
    assert!(
        format!("{error:?}").contains("same number of days"),
        "{error:?}"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn an_empty_history_is_refused() {
    let (runtime, path) = runtime();
    runtime
        .admit_cycle_stages(Vec::new(), Vec::new(), Vec::new(), None, None, None)
        .expect_err("an empty history cannot found a prediction");
    let _ = std::fs::remove_file(path);
}

/// Asking twice with the same history queues once. The second pass is answered from the cache,
/// and still carries the health of the input that produced it.
#[test]
fn the_same_history_is_not_queued_twice() {
    let (runtime, path) = runtime();
    let (temperature, breath, heart) = plausible(40);
    let first = runtime
        .admit_cycle_stages(
            temperature.clone(),
            breath.clone(),
            heart.clone(),
            Some(31.0),
            Some(29.0),
            Some(13.0),
        )
        .expect("first");
    let outstanding = runtime.outstanding_model_inferences().expect("outstanding");
    assert_eq!(outstanding, 6);

    // Nothing has answered, so the fingerprints are issued-but-not-fresh and the second pass
    // must not double-queue them.
    let second = runtime
        .admit_cycle_stages(
            temperature,
            breath,
            heart,
            Some(31.0),
            Some(29.0),
            Some(13.0),
        )
        .expect("second");
    assert_eq!(first.len(), second.len());
    assert_eq!(
        runtime.outstanding_model_inferences().expect("outstanding"),
        6,
        "the second pass queued the same work again"
    );
    let _ = std::fs::remove_file(path);
}

/// The same rule for the tensors-in-hand path, which used to check only what had *completed*.
///
/// Two planning passes can overlap — a foreground resume landing on top of a background window —
/// and both would see the stage as not-yet-answered, because the first one's inference is still
/// in flight. Without the in-flight check the accelerator runs the same tensors twice and one of
/// the two results is thrown away.
#[test]
fn a_stage_already_in_flight_is_not_queued_a_second_time() {
    let (runtime, path) = runtime();
    let inputs = || {
        vec![ModelTensor {
            name: "ppg".to_owned(),
            values: vec![0.25; 12_000],
        }]
    };

    let first = runtime
        .admit_analytics_stage("pulse_ppg".to_owned(), inputs())
        .expect("first");
    assert!(first.request_id.is_some());
    assert!(!first.already_known);
    assert_eq!(runtime.outstanding_model_inferences().expect("count"), 1);

    let second = runtime
        .admit_analytics_stage("pulse_ppg".to_owned(), inputs())
        .expect("second");
    assert_eq!(second.request_id, None, "the same tensors queued twice");
    assert!(
        !second.already_known,
        "in flight is not the same as answered, and a surface reads the difference"
    );
    assert_eq!(
        runtime.outstanding_model_inferences().expect("count"),
        1,
        "the accelerator was asked for the same inference twice"
    );

    // Different tensors for the same model are a different question and still queue.
    let other = runtime
        .admit_analytics_stage(
            "pulse_ppg".to_owned(),
            vec![ModelTensor {
                name: "ppg".to_owned(),
                values: vec![0.5; 12_000],
            }],
        )
        .expect("other");
    assert!(other.request_id.is_some());
    assert_eq!(runtime.outstanding_model_inferences().expect("count"), 2);
    let _ = std::fs::remove_file(path);
}
