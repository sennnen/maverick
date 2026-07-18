use crate::abi::{
    ActionBatch, ActionBody, ConnectorAction, ConnectorEvent, OperationId, TimerToken, MAX_ACTIONS,
};
use crate::ConnectorError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionBuilder {
    event: ConnectorEvent,
    actions: Vec<ConnectorAction>,
}

impl ActionBuilder {
    pub fn for_event(event: &ConnectorEvent) -> Self {
        Self {
            event: event.clone(),
            actions: Vec::new(),
        }
    }

    pub fn push(
        mut self,
        operation_id: OperationId,
        deadline_token: TimerToken,
        body: ActionBody,
    ) -> Result<Self, ConnectorError> {
        if self.actions.len() == MAX_ACTIONS {
            return Err(ConnectorError::TooManyActions { limit: MAX_ACTIONS });
        }
        self.actions.push(ConnectorAction {
            connector_id: self.event.connector_id.clone(),
            session_id: self.event.session_id,
            caused_by: self.event.sequence,
            cancellation_generation: self.event.cancellation_generation,
            operation_id,
            deadline_token,
            body,
        });
        Ok(self)
    }

    pub fn finish(self) -> Result<ActionBatch, ConnectorError> {
        let batch = ActionBatch {
            actions: self.actions,
        };
        crate::abi::Validate::validate(&batch)?;
        Ok(batch)
    }
}
