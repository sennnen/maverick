#![allow(clippy::expect_used, clippy::panic)]

mod common;

use common::{artifact, event, module, valid_module};
use mav_connector_abi::{encode_canonical, ActionBatch, EventBody, MAX_EVENT_BYTES};
use mav_connector_runtime::{ConnectorInstance, LimitProfile};
use mav_model::error::{codes, Result};

fn code<T>(result: Result<T>) -> u16 {
    match result {
        Ok(_) => 0,
        Err(error) => error.code,
    }
}

#[test]
fn valid_instance_runs_init_handle_snapshot_and_repeats_exactly() {
    let good_artifact = artifact(valid_module());
    let profile = LimitProfile::mobile_v1();
    let mut first =
        ConnectorInstance::instantiate(&good_artifact, profile.clone()).expect("instance");
    let mut second =
        ConnectorInstance::instantiate(&good_artifact, profile).expect("second instance");
    let expected = ActionBatch {
        actions: Vec::new(),
    };
    assert_eq!(first.init(&event()), Ok(expected.clone()));
    assert_eq!(first.handle(&event()), Ok(expected.clone()));
    assert_eq!(second.handle(&event()), Ok(expected));
    assert_eq!(first.snapshot(), Ok(vec![1, 2, 3]));
    assert!(first.is_usable());
    assert_eq!(
        good_artifact
            .run_fixtures(LimitProfile::mobile_v1())
            .expect("embedded fixtures")[0],
        mav_connector_runtime::FixtureResult {
            name: "activate".to_owned(),
            events_run: 1,
            input_hash: [
                0xf4, 0x16, 0x6c, 0x62, 0x5c, 0x17, 0x89, 0x17, 0xa9, 0xe1, 0x47, 0xd3, 0x88, 0xe0,
                0x95, 0x5b, 0xfa, 0x5d, 0x56, 0x1c, 0x01, 0xb9, 0xe9, 0xdc, 0xe4, 0x0c, 0xe6, 0x23,
                0x39, 0x29, 0x12, 0x7b,
            ],
            action_trace_hash: [
                0x10, 0x96, 0x8e, 0x27, 0x01, 0x99, 0xfd, 0xe3, 0x9c, 0xb0, 0xa3, 0xfc, 0x18, 0x81,
                0x89, 0xca, 0x72, 0x44, 0x8e, 0x53, 0xef, 0xb8, 0x46, 0x7d, 0xdd, 0xdb, 0xd0, 0x45,
                0xb6, 0x74, 0x3e, 0x6d,
            ],
            sample_hash: [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55,
            ],
            final_state_hash: [
                0x03, 0x90, 0x58, 0xc6, 0xf2, 0xc0, 0xcb, 0x49, 0x2c, 0x53, 0x3b, 0x0a, 0x4d, 0x14,
                0xef, 0x77, 0xcc, 0x0f, 0x78, 0xab, 0xcc, 0xce, 0xd5, 0x28, 0x7d, 0x84, 0xa1, 0xa2,
                0x01, 0x1c, 0xfb, 0x81,
            ],
            max_fuel_consumed: 8,
            peak_memory_bytes: 131_072,
        }
    );
}

#[test]
fn imports_start_shared_memory_and_wrong_exports_reject_before_execution() {
    let imported = wat::parse_str(
        r#"(module
            (import "wasi_snapshot_preview1" "clock_time_get" (func))
            (memory (export "memory") 1)
        )"#,
    )
    .expect("import module");
    assert_eq!(
        code(ConnectorInstance::instantiate(
            &artifact(imported),
            LimitProfile::mobile_v1()
        )),
        codes::CONNECTOR_RUNTIME_IMPORT_FORBIDDEN
    );

    let started =
        wat::parse_str("(module (func $start) (start $start) (memory (export \"memory\") 1))")
            .expect("start module");
    assert_eq!(
        code(ConnectorInstance::instantiate(
            &artifact(started),
            LimitProfile::mobile_v1()
        )),
        codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN
    );

    let shared =
        wat::parse_str("(module (memory (export \"memory\") 1 2 shared))").expect("shared module");
    assert_eq!(
        code(ConnectorInstance::instantiate(
            &artifact(shared),
            LimitProfile::mobile_v1()
        )),
        codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN
    );

    let references = wat::parse_str(
        "(module (memory (export \"memory\") 1) (func (result externref) ref.null extern))",
    )
    .expect("reference module");
    assert_eq!(
        code(ConnectorInstance::instantiate(
            &artifact(references),
            LimitProfile::mobile_v1()
        )),
        codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN
    );

    let reference_global = wat::parse_str(
        "(module (memory (export \"memory\") 1) (global externref (ref.null extern)))",
    )
    .expect("reference global module");
    assert_eq!(
        code(ConnectorInstance::instantiate(
            &artifact(reference_global),
            LimitProfile::mobile_v1()
        )),
        codes::CONNECTOR_RUNTIME_FEATURE_FORBIDDEN
    );

    let wrong = wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "mav_abi_version") (result i32) i32.const 1)
        )"#,
    )
    .expect("wrong exports");
    assert_eq!(
        code(ConnectorInstance::instantiate(
            &artifact(wrong),
            LimitProfile::mobile_v1()
        )),
        codes::CONNECTOR_RUNTIME_EXPORT_INVALID
    );
}

#[test]
fn fuel_stack_memory_and_module_limits_are_typed() {
    let output = encode_canonical(&ActionBatch {
        actions: Vec::new(),
    })
    .expect("batch encode");
    let looping = artifact(module("(loop $spin br $spin) i64.const 0", &output, &[]));
    let mut instance =
        ConnectorInstance::instantiate(&looping, LimitProfile::mobile_v1()).expect("instance");
    assert_eq!(
        code(instance.handle(&event())),
        codes::CONNECTOR_RUNTIME_FUEL_EXHAUSTED
    );
    assert!(!instance.is_usable());

    let recursive = artifact(module("local.get 0 local.get 1 call $handle", &output, &[]));
    let mut instance =
        ConnectorInstance::instantiate(&recursive, LimitProfile::mobile_v1()).expect("instance");
    assert_eq!(
        code(instance.handle(&event())),
        codes::CONNECTOR_RUNTIME_STACK_LIMIT
    );

    let growing = artifact(module(
        "i32.const 63 memory.grow drop i64.const 0",
        &output,
        &[],
    ));
    let mut instance =
        ConnectorInstance::instantiate(&growing, LimitProfile::mobile_v1()).expect("instance");
    assert_eq!(
        code(instance.handle(&event())),
        codes::CONNECTOR_RUNTIME_RESOURCE_LIMIT
    );

    let table_bomb = wat::parse_str("(module (table 1025 funcref) (memory (export \"memory\") 1))")
        .expect("table bomb");
    assert_eq!(
        code(ConnectorInstance::instantiate(
            &artifact(table_bomb),
            LimitProfile::mobile_v1()
        )),
        codes::CONNECTOR_RUNTIME_MODULE_LIMIT
    );
}

#[test]
fn invalid_pointers_output_bombs_and_malformed_output_fail_closed() {
    let empty = encode_canonical(&ActionBatch {
        actions: Vec::new(),
    })
    .expect("batch encode");
    let invalid_pointer = ((131_060_u64) << 32) | 100;
    let mut instance = ConnectorInstance::instantiate(
        &artifact(module(&format!("i64.const {invalid_pointer}"), &empty, &[])),
        LimitProfile::mobile_v1(),
    )
    .expect("instance");
    assert_eq!(
        code(instance.handle(&event())),
        codes::CONNECTOR_RUNTIME_MEMORY_ACCESS
    );

    let bomb = ((1_024_u64) << 32) | 1_048_577;
    let mut instance = ConnectorInstance::instantiate(
        &artifact(module(&format!("i64.const {bomb}"), &empty, &[])),
        LimitProfile::mobile_v1(),
    )
    .expect("instance");
    assert_eq!(
        code(instance.handle(&event())),
        codes::CONNECTOR_RUNTIME_OUTPUT_OVERSIZED
    );

    let malformed = ((1_024_u64) << 32) | 1;
    let mut instance = ConnectorInstance::instantiate(
        &artifact(module(&format!("i64.const {malformed}"), &[0xff], &[])),
        LimitProfile::mobile_v1(),
    )
    .expect("instance");
    assert_eq!(
        code(instance.handle(&event())),
        codes::CONNECTOR_RUNTIME_OUTPUT_INVALID
    );
}

#[test]
fn input_state_bounds_and_trap_isolation_hold() {
    let good_artifact = artifact(valid_module());
    let mut instance = ConnectorInstance::instantiate(&good_artifact, LimitProfile::mobile_v1())
        .expect("instance");
    let mut oversized = event();
    oversized.body = EventBody::Notification {
        characteristic_id: "data".to_owned(),
        bytes: vec![0; MAX_EVENT_BYTES],
    };
    assert_eq!(
        code(instance.handle(&oversized)),
        codes::CONNECTOR_RUNTIME_INPUT_OVERSIZED
    );

    let state = vec![1; 65_537];
    let mut state_instance = ConnectorInstance::instantiate(
        &artifact(module("i64.const 0", &[], &state)),
        LimitProfile::mobile_v1(),
    )
    .expect("state instance");
    assert_eq!(
        code(state_instance.snapshot()),
        codes::CONNECTOR_RUNTIME_STATE_OVERSIZED
    );

    let trapped = artifact(module("unreachable", &[], &[]));
    let mut bad =
        ConnectorInstance::instantiate(&trapped, LimitProfile::mobile_v1()).expect("bad instance");
    let mut good = ConnectorInstance::instantiate(&good_artifact, LimitProfile::mobile_v1())
        .expect("good instance");
    assert_eq!(code(bad.handle(&event())), codes::CONNECTOR_RUNTIME_TRAP);
    assert_eq!(
        good.handle(&event()),
        Ok(ActionBatch {
            actions: Vec::new()
        })
    );
    assert!(good.is_usable());
}

/// The ABI's two snapshot sentinels must stay distinguishable. Zero is a legally empty snapshot;
/// -1 is a guest saying it could not build one. Collapsing them turns a failure into empty state,
/// which the host then persists as though it were the truth.
#[test]
fn a_failed_snapshot_is_an_error_and_an_empty_one_is_not() {
    let mut failing = ConnectorInstance::instantiate(
        &artifact(common::module_with_snapshot(
            "i64.const 0",
            &[],
            &[],
            Some(-1),
        )),
        LimitProfile::mobile_v1(),
    )
    .expect("failing instance");
    assert_eq!(
        code(failing.snapshot()),
        codes::CONNECTOR_RUNTIME_SNAPSHOT_FAILED
    );

    let mut empty = ConnectorInstance::instantiate(
        &artifact(common::module_with_snapshot(
            "i64.const 0",
            &[],
            &[],
            Some(0),
        )),
        LimitProfile::mobile_v1(),
    )
    .expect("empty instance");
    assert_eq!(empty.snapshot(), Ok(Vec::new()));
}
