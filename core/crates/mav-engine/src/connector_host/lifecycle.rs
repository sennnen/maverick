//! Event admission, the lifecycle a batch would produce, and result correlation.

use super::*;

impl ConnectorHost {
    pub(super) fn simulate_action(
        &self,
        body: &ActionBody,
        lifecycle: &mut ConnectorLifecycle,
    ) -> Result<()> {
        match body {
            ActionBody::StartScan { .. }
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Selected | ConnectorLifecycle::Disconnected
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Scanning;
            }
            ActionBody::StopScan if *lifecycle == ConnectorLifecycle::Scanning => {}
            ActionBody::Connect { .. } if *lifecycle == ConnectorLifecycle::Scanning => {
                *lifecycle = ConnectorLifecycle::Connecting;
            }
            ActionBody::EnsurePaired
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Connecting | ConnectorLifecycle::Configuring
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Pairing;
            }
            ActionBody::DiscoverServices
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Connecting
                        | ConnectorLifecycle::Pairing
                        | ConnectorLifecycle::Configuring
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Discovering;
            }
            ActionBody::Subscribe { .. }
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Discovering | ConnectorLifecycle::Configuring
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Configuring;
            }
            ActionBody::Unsubscribe { .. } | ActionBody::Read { .. } | ActionBody::Write { .. }
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Configuring
                        | ConnectorLifecycle::Streaming
                        | ConnectorLifecycle::Historical
                ) => {}
            ActionBody::SetTimer { .. } | ActionBody::CancelTimer { .. } => {}
            ActionBody::DeclareCapabilities { .. }
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Configuring | ConnectorLifecycle::Streaming
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Streaming;
            }
            ActionBody::Disconnect
                if !matches!(
                    lifecycle,
                    ConnectorLifecycle::Installed | ConnectorLifecycle::Disconnected
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Suspending;
            }
            body if !is_transport(body) => {}
            _ => {
                return Err(host_state(
                    "connector action is invalid in the current lifecycle state",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn accept_event(
        &mut self,
        body: &mut EventBody,
        wall_time_ms: Option<i64>,
    ) -> Result<bool> {
        let allowed = match body {
            EventBody::Advertisement { address, .. }
                if self.lifecycle == ConnectorLifecycle::Scanning =>
            {
                self.advertised_addresses.insert(address.clone());
                if self.advertised_addresses.len() > MAX_ADVERTISED_ADDRESSES {
                    return Err(host_state(
                        "connector session advertisement budget exhausted",
                    ));
                }
                true
            }
            EventBody::Connected { .. } if self.lifecycle == ConnectorLifecycle::Connecting => true,
            EventBody::PairingResult { .. } if self.lifecycle == ConnectorLifecycle::Pairing => {
                self.lifecycle = ConnectorLifecycle::Configuring;
                true
            }
            EventBody::ServicesDiscovered { .. }
                if self.lifecycle == ConnectorLifecycle::Discovering =>
            {
                self.lifecycle = ConnectorLifecycle::Configuring;
                true
            }
            EventBody::Subscribed { characteristic_id }
                if matches!(
                    self.lifecycle,
                    ConnectorLifecycle::Configuring | ConnectorLifecycle::Streaming
                ) =>
            {
                self.characteristic(characteristic_id)?;
                true
            }
            EventBody::Unsubscribed { characteristic_id }
                if matches!(
                    self.lifecycle,
                    ConnectorLifecycle::Configuring | ConnectorLifecycle::Streaming
                ) =>
            {
                self.characteristic(characteristic_id)?;
                true
            }
            EventBody::Notification {
                characteristic_id, ..
            } if matches!(
                self.lifecycle,
                ConnectorLifecycle::Configuring
                    | ConnectorLifecycle::Streaming
                    | ConnectorLifecycle::Historical
            ) =>
            {
                self.characteristic(characteristic_id)?;
                true
            }
            EventBody::ReadResult {
                operation_id,
                characteristic_id,
                ..
            } => self.take_expected(operation_id, characteristic_id, true, wall_time_ms)?,
            EventBody::WriteResult {
                operation_id,
                characteristic_id,
            } => self.take_expected(operation_id, characteristic_id, false, wall_time_ms)?,
            EventBody::TimerFired { token } => {
                if self.pending_timers.remove(&token.0) {
                    true
                } else {
                    self.record_late("late or cancelled timer result", wall_time_ms)?;
                    false
                }
            }
            EventBody::TransportError {
                operation_id: Some(operation_id),
                ..
            } => {
                if let Some(expected) = self.outstanding.remove(&operation_id.0) {
                    operation_id.0 = expected.connector_operation_id();
                    true
                } else {
                    self.record_late("late transport error", wall_time_ms)?;
                    false
                }
            }
            EventBody::TransportError {
                operation_id: None, ..
            } => true,
            EventBody::Disconnected { .. } => {
                self.cancellation_generation = self
                    .cancellation_generation
                    .checked_add(1)
                    .ok_or_else(|| host_state("connector cancellation generation exhausted"))?;
                self.actions.clear();
                self.outstanding.clear();
                self.pending_timers.clear();
                self.active_capabilities.clear();
                self.active_capture = None;
                if let Some(capture) = self.ecg_capture.as_mut() {
                    capture.cancel();
                }
                self.lifecycle = ConnectorLifecycle::Disconnected;
                true
            }
            EventBody::ScanStopped { .. }
                if matches!(
                    self.lifecycle,
                    ConnectorLifecycle::Scanning | ConnectorLifecycle::Connecting
                ) =>
            {
                true
            }
            EventBody::MtuChanged { .. }
                if matches!(
                    self.lifecycle,
                    ConnectorLifecycle::Connecting
                        | ConnectorLifecycle::Pairing
                        | ConnectorLifecycle::Discovering
                        | ConnectorLifecycle::Configuring
                        | ConnectorLifecycle::Streaming
                        | ConnectorLifecycle::Historical
                ) =>
            {
                true
            }
            _ => {
                return Err(host_state(
                    "transport event is invalid in the current lifecycle state",
                ));
            }
        };
        Ok(allowed)
    }

    pub(super) fn take_expected(
        &mut self,
        operation_id: &mut OperationId,
        characteristic_id: &str,
        read: bool,
        wall_time_ms: Option<i64>,
    ) -> Result<bool> {
        let Some(expected) = self.outstanding.get(&operation_id.0) else {
            self.record_late("late or cancelled transport result", wall_time_ms)?;
            return Ok(false);
        };
        let matches = match expected {
            ExpectedResult::Read {
                characteristic_id: expected,
                ..
            } => read && expected == characteristic_id,
            ExpectedResult::Write {
                characteristic_id: expected,
                ..
            } => !read && expected == characteristic_id,
        };
        if !matches {
            return Err(error(
                codes::CONNECTOR_HOST_RESULT_MISMATCH,
                "transport result differs from its pending operation",
            ));
        }
        let expected = self
            .outstanding
            .remove(&operation_id.0)
            .ok_or_else(|| host_state("pending operation disappeared during result mapping"))?;
        operation_id.0 = expected.connector_operation_id();
        Ok(true)
    }

    pub(super) fn record_late(&self, message: &str, wall_time_ms: Option<i64>) -> Result<()> {
        self.store.record_error(
            &error(codes::CONNECTOR_HOST_LATE_RESULT, message),
            wall_time_ms.unwrap_or_default().saturating_mul(1_000_000),
        )
    }
}

pub(super) fn transition_for_action(
    current: ConnectorLifecycle,
    body: &ActionBody,
) -> ConnectorLifecycle {
    match body {
        ActionBody::StartScan { .. } => ConnectorLifecycle::Scanning,
        ActionBody::Connect { .. } => ConnectorLifecycle::Connecting,
        ActionBody::EnsurePaired => ConnectorLifecycle::Pairing,
        ActionBody::DiscoverServices => ConnectorLifecycle::Discovering,
        ActionBody::Subscribe { .. } => ConnectorLifecycle::Configuring,
        ActionBody::Disconnect => ConnectorLifecycle::Suspending,
        _ => current,
    }
}

pub(super) fn is_transport(body: &ActionBody) -> bool {
    matches!(
        body,
        ActionBody::StartScan { .. }
            | ActionBody::StopScan
            | ActionBody::Connect { .. }
            | ActionBody::EnsurePaired
            | ActionBody::DiscoverServices
            | ActionBody::Subscribe { .. }
            | ActionBody::Unsubscribe { .. }
            | ActionBody::Read { .. }
            | ActionBody::Write { .. }
            | ActionBody::Disconnect
            | ActionBody::SetTimer { .. }
            | ActionBody::CancelTimer { .. }
    )
}
