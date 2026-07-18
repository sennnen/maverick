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
            .expect("embedded fixtures")[0]
            .events_run,
        1
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
