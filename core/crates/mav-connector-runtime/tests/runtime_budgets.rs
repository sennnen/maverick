#![allow(clippy::expect_used, clippy::panic)]

mod common;

use common::{artifact, event, valid_module};
use mav_connector_abi::EventBody;
use mav_connector_runtime::{ConnectorInstance, LimitProfile};
use std::time::Instant;

#[test]
fn p0_latency_budgets_remain_green() {
    if cfg!(debug_assertions) {
        assert_eq!(LimitProfile::mobile_v1().id(), "mobile-v1");
        return;
    }
    let artifact = artifact(valid_module());
    let profile = LimitProfile::mobile_v1();
    let mut event = event();
    event.body = EventBody::Notification {
        characteristic_id: "data".to_owned(),
        bytes: vec![0x5a; 8 * 1024],
    };
    let cold_start = Instant::now();
    for _ in 0..200 {
        let mut instance =
            ConnectorInstance::instantiate(&artifact, profile.clone()).expect("cold instance");
        instance.handle(&event).expect("cold handle");
    }
    let cold_mean_us = cold_start.elapsed().as_micros() as u64 / 200;

    let mut instance = ConnectorInstance::instantiate(&artifact, profile).expect("warm instance");
    let mut warm_us = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let start = Instant::now();
        instance.handle(&event).expect("warm handle");
        warm_us.push(start.elapsed().as_nanos() as u64 / 1_000);
    }
    warm_us.sort_unstable();
    let warm_p95_us = warm_us[9_499];
    println!("cold_mean_us={cold_mean_us} warm_p95_us={warm_p95_us}");
    assert!(
        cold_mean_us <= 250,
        "cold mean {cold_mean_us} us exceeds 250 us"
    );
    assert!(
        warm_p95_us <= 250,
        "warm p95 {warm_p95_us} us exceeds 250 us"
    );
}
