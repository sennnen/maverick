//! Action validation, transport translation, and the host-assigned operation and deadline ids.

use super::*;

impl ConnectorHost {
    pub(super) fn validate_action(
        &self,
        action: &ConnectorAction,
        caused_by: EventSequence,
        operations: &mut BTreeSet<u64>,
        deadlines: &mut BTreeSet<u64>,
    ) -> Result<()> {
        if action.connector_id != self.connector_id
            || action.session_id.0 != self.config.session_id
            || action.caused_by != caused_by
            || action.cancellation_generation.0 != self.cancellation_generation
        {
            return Err(error(
                codes::CONNECTOR_HOST_ACTION_INVALID,
                "connector action context differs from the active event",
            ));
        }
        if action.operation_id.0 == 0
            || !operations.insert(action.operation_id.0)
            || action.deadline_token.0 == 0
            || !deadlines.insert(action.deadline_token.0)
        {
            return Err(error(
                codes::CONNECTOR_HOST_OPERATION_DUPLICATE,
                "connector operation and deadline ids must be positive and session-unique",
            ));
        }
        self.validate_declared(&action.body)
    }

    pub(super) fn execute_action(
        &mut self,
        action: ConnectorAction,
        wall_time_ms: Option<i64>,
        followups: &mut Vec<EventBody>,
    ) -> Result<()> {
        let connector_operation_id = action.operation_id.0;
        match action.body {
            ActionBody::StatePut { key, value } => {
                self.staged_state.insert(key, Some(value));
            }
            ActionBody::StateDelete { key } => {
                self.staged_state.insert(key, None);
            }
            ActionBody::StateCommit => {
                for (key, value) in std::mem::take(&mut self.staged_state) {
                    match value {
                        Some(value) => {
                            self.committed_state.insert(key, value);
                        }
                        None => {
                            self.committed_state.remove(&key);
                        }
                    }
                }
                self.state_revision = self
                    .state_revision
                    .checked_add(1)
                    .ok_or_else(|| host_state("connector state revision exhausted"))?;
                followups.push(EventBody::StateCommitted {
                    revision: self.state_revision,
                });
            }
            ActionBody::EmitSamples { batch_id, samples } => {
                let accounting = self.commit_samples(batch_id, &samples, wall_time_ms)?;
                if accounting.duplicate > 0 {
                    // Not an error, but never silent: a repeated historical burst is expected and a
                    // sample that vanishes without being counted here is not.
                    self.record_duplicates(&accounting, wall_time_ms)?;
                }
                // The acknowledgement is what the connector handed over. It means received and
                // safely handled, which includes recognised as already held.
                let count = u32::try_from(accounting.emitted).map_err(|_| {
                    error(
                        codes::CONNECTOR_HOST_SAMPLE_INVALID,
                        "connector sample acknowledgment count exceeds ABI width",
                    )
                })?;
                followups.push(EventBody::SamplesCommitted { batch_id, count });
            }
            ActionBody::EmitDiagnostic { code, .. } => {
                let diagnostic = error(
                    codes::CONNECTOR_HOST_ACTION_INVALID,
                    "connector emitted a diagnostic",
                )
                .context(code);
                self.store.record_error(
                    &diagnostic,
                    wall_time_ms.unwrap_or_default().saturating_mul(1_000_000),
                )?;
            }
            ActionBody::DeclareCapabilities { .. } => {
                self.lifecycle = ConnectorLifecycle::Streaming;
            }
            ActionBody::CompleteOperation {
                operation_id: completed,
            } => {
                self.outstanding
                    .retain(|_, expected| expected.connector_operation_id() != completed.0);
            }
            body => {
                let operation_id = self.allocate_host_operation_id()?;
                let deadline_token = self.allocate_host_deadline_token()?;
                if let ActionBody::Read { characteristic_id } = &body {
                    self.outstanding.insert(
                        operation_id,
                        ExpectedResult::Read {
                            characteristic_id: characteristic_id.clone(),
                            connector_operation_id,
                        },
                    );
                }
                if let ActionBody::Write {
                    characteristic_id, ..
                } = &body
                {
                    self.outstanding.insert(
                        operation_id,
                        ExpectedResult::Write {
                            characteristic_id: characteristic_id.clone(),
                            connector_operation_id,
                        },
                    );
                }
                if let ActionBody::SetTimer { token, .. } = &body {
                    self.pending_timers.insert(token.0);
                }
                if let ActionBody::CancelTimer { token } = &body {
                    self.pending_timers.remove(&token.0);
                }
                self.lifecycle = transition_for_action(self.lifecycle, &body);
                self.actions.push_back(ConnectorTransportAction {
                    connector_id: self.connector_id.clone(),
                    session_id: self.config.session_id,
                    cancellation_generation: self.cancellation_generation,
                    operation_id,
                    deadline_token,
                    body: transport_request(body)?,
                });
            }
        }
        Ok(())
    }

    pub(super) fn allocate_host_operation_id(&mut self) -> Result<u64> {
        let value = self.next_host_operation_id;
        self.next_host_operation_id = value
            .checked_add(1)
            .ok_or_else(|| host_state("host operation id exhausted"))?;
        Ok(value)
    }

    pub(super) fn validate_state_batch(&self, batch: &ActionBatch) -> Result<()> {
        let mut staged = self.staged_state.clone();
        let mut committed = self.committed_state.clone();
        for action in &batch.actions {
            match &action.body {
                ActionBody::StatePut { key, value } => {
                    staged.insert(key.clone(), Some(value.clone()));
                }
                ActionBody::StateDelete { key } => {
                    staged.insert(key.clone(), None);
                }
                ActionBody::StateCommit => {
                    for (key, value) in std::mem::take(&mut staged) {
                        match value {
                            Some(value) => {
                                committed.insert(key, value);
                            }
                            None => {
                                committed.remove(&key);
                            }
                        }
                    }
                    if state_bytes(&committed) > MAX_STATE_BYTES {
                        return Err(error(
                            codes::CONNECTOR_HOST_ACTION_INVALID,
                            "connector committed state exceeds the session bound",
                        ));
                    }
                }
                _ => {}
            }
            if staged_state_bytes(&staged) > MAX_STATE_BYTES {
                return Err(error(
                    codes::CONNECTOR_HOST_ACTION_INVALID,
                    "connector staged state exceeds the session bound",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn allocate_host_deadline_token(&mut self) -> Result<u64> {
        let value = self.next_host_deadline_token;
        self.next_host_deadline_token = value
            .checked_add(1)
            .ok_or_else(|| host_state("host deadline token exhausted"))?;
        Ok(value)
    }
}

pub(super) fn transport_request(body: ActionBody) -> Result<ConnectorTransportRequest> {
    let request = match body {
        ActionBody::StartScan {
            service_uuids,
            manufacturer_ids,
        } => ConnectorTransportRequest::StartScan {
            service_uuids,
            manufacturer_ids,
        },
        ActionBody::StopScan => ConnectorTransportRequest::StopScan,
        ActionBody::Connect { address } => ConnectorTransportRequest::Connect { address },
        ActionBody::EnsurePaired => ConnectorTransportRequest::EnsurePaired,
        ActionBody::DiscoverServices => ConnectorTransportRequest::DiscoverServices,
        ActionBody::Subscribe { characteristic_id } => {
            ConnectorTransportRequest::Subscribe { characteristic_id }
        }
        ActionBody::Unsubscribe { characteristic_id } => {
            ConnectorTransportRequest::Unsubscribe { characteristic_id }
        }
        ActionBody::Read { characteristic_id } => {
            ConnectorTransportRequest::Read { characteristic_id }
        }
        ActionBody::Write {
            characteristic_id,
            bytes,
            confirmed,
        } => ConnectorTransportRequest::Write {
            characteristic_id,
            bytes,
            confirmed,
        },
        ActionBody::Disconnect => ConnectorTransportRequest::Disconnect,
        ActionBody::SetTimer { token, delay_ms } => ConnectorTransportRequest::SetTimer {
            token: token.0,
            delay_ms,
        },
        ActionBody::CancelTimer { token } => {
            ConnectorTransportRequest::CancelTimer { token: token.0 }
        }
        _ => {
            return Err(error(
                codes::CONNECTOR_HOST_ACTION_INVALID,
                "non-transport action reached the transport queue",
            ));
        }
    };
    Ok(request)
}

pub(super) fn state_bytes(values: &BTreeMap<String, Vec<u8>>) -> usize {
    values.iter().fold(0_usize, |total, (key, value)| {
        total.saturating_add(key.len()).saturating_add(value.len())
    })
}

pub(super) fn staged_state_bytes(values: &BTreeMap<String, Option<Vec<u8>>>) -> usize {
    values.iter().fold(0_usize, |total, (key, value)| {
        total
            .saturating_add(key.len())
            .saturating_add(value.as_deref().map_or(0, <[u8]>::len))
    })
}
