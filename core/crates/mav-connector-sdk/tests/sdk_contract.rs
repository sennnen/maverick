#![allow(clippy::expect_used, clippy::panic)]

use mav_connector_sdk::abi::*;
use mav_connector_sdk::{ActionBuilder, Connector, ConnectorError, TestDriver, ABI_VERSION};

#[derive(Default)]
struct Example;

impl Connector for Example {
    fn handle(&mut self, event: ConnectorEvent) -> Result<ActionBatch, ConnectorError> {
        ActionBuilder::for_event(&event)
            .push(
                OperationId(9),
                TimerToken(10),
                ActionBody::CompleteOperation {
                    operation_id: OperationId(9),
                },
            )?
            .finish()
    }

    fn snapshot(&self) -> Result<Vec<u8>, ConnectorError> {
        Ok(vec![1, 2, 3])
    }
}

fn event() -> ConnectorEvent {
    ConnectorEvent {
        connector_id: ConnectorId::new("org.example.template").expect("connector id"),
        session_id: SessionId(7),
        sequence: EventSequence(8),
        cancellation_generation: CancellationGeneration(2),
        wall_time_ms: None,
        body: EventBody::Activate,
    }
}

#[test]
fn builder_copies_event_context_and_driver_asserts_exact_output() {
    let expected = ActionBatch {
        actions: vec![ConnectorAction {
            connector_id: ConnectorId::new("org.example.template").expect("connector id"),
            session_id: SessionId(7),
            caused_by: EventSequence(8),
            cancellation_generation: CancellationGeneration(2),
            operation_id: OperationId(9),
            deadline_token: TimerToken(10),
            body: ActionBody::CompleteOperation {
                operation_id: OperationId(9),
            },
        }],
    };
    let mut driver = TestDriver::new(Example);
    assert_eq!(driver.drive(event()), Ok(expected));
    assert_eq!(driver.snapshot(), Ok(vec![1, 2, 3]));
    assert_eq!(ABI_VERSION, pack_ptr_len(1, 0));
}

#[test]
fn builder_rejects_action_overflow_exactly() {
    let mut builder = ActionBuilder::for_event(&event());
    for index in 0..MAX_ACTIONS {
        builder = builder
            .push(
                OperationId(index as u64),
                TimerToken(1),
                ActionBody::StopScan,
            )
            .expect("bounded action");
    }
    assert_eq!(
        builder.push(OperationId(99), TimerToken(1), ActionBody::StopScan),
        Err(ConnectorError::TooManyActions { limit: MAX_ACTIONS })
    );
}
