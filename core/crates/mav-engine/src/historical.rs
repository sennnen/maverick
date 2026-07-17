//! Fail-closed orchestration for a historical transfer.
//!
//! The controller owns protocol order, sequence matching, and the safe-ack boundary. It does not
//! own BLE, storage, decoding, or time. Most importantly, it cannot acknowledge a burst until its
//! caller reports that the burst committed durably.

use crate::acquisition::Command;
use crate::recompute::AffectedDays;
use mav_model::error::{codes, MavError, Result};
use mav_obs::stage::Stage;
use mav_obs::tap::{Ids, Tap, TapEvent};
use serde::Serialize;

pub const HISTORICAL_STATUS_SCHEMA: &str = "historical-status/v1";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HistoricalState {
    Idle,
    AwaitingRange,
    AwaitingSendAcceptance,
    Receiving,
    AwaitingDurableCommit,
    Complete,
    Failed,
}

impl HistoricalState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Idle => "historical_idle",
            Self::AwaitingRange => "historical_awaiting_range",
            Self::AwaitingSendAcceptance => "historical_awaiting_send_acceptance",
            Self::Receiving => "historical_receiving",
            Self::AwaitingDurableCommit => "historical_awaiting_durable_commit",
            Self::Complete => "historical_complete",
            Self::Failed => "historical_failed",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResponseResult {
    Ok,
    Pending,
    Unknown(u8),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommandTemplate {
    pub opcode: u8,
    pub b3: Option<u8>,
    pub payload: Vec<u8>,
}

impl CommandTemplate {
    fn command(&self, seq: u8) -> Command {
        Command {
            opcode: self.opcode,
            seq,
            b3: self.b3,
            payload: self.payload.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HistoricalConfig {
    pub get_data_range: CommandTemplate,
    pub send_historical: CommandTemplate,
    pub acknowledge: CommandTemplate,
    pub max_retries: u8,
    pub max_ack_payload_bytes: usize,
}

#[derive(Clone, PartialEq, Debug)]
pub enum HistoricalEvent {
    Start,
    Response {
        to_opcode: u8,
        origin_seq: u8,
        result: ResponseResult,
    },
    BurstStarted,
    BurstEnded {
        ack_payload: Vec<u8>,
        record_count: u32,
    },
    BurstPersisted,
    PersistFailed {
        error: MavError,
    },
    HistoryComplete,
    Timeout,
    Disconnect,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct HistoricalOutcome {
    pub commands: Vec<Command>,
}

/// Sums the burst receipts and decode rejections of one sync, for the progress report. It holds
/// outcomes only — no cursor bytes and no commands — so handing it to a host reveals nothing the
/// host could replay at the device.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SyncTotals {
    inserted: u64,
    duplicates: u64,
    rejected_records: u64,
    days: AffectedDays,
}

impl SyncTotals {
    pub fn absorb(&mut self, receipt: &crate::burst::BurstReceipt) {
        self.inserted += u64::from(receipt.inserted);
        self.duplicates += u64::from(receipt.duplicates);
        self.days.union(&receipt.affected_days);
    }

    pub fn note_rejected(&mut self, count: u64) {
        self.rejected_records += count;
    }

    pub fn affected_days(&self) -> &AffectedDays {
        &self.days
    }
}

/// The progress and failure read model the FFI hands to hosts: honest counts, the durable cursor
/// as a hash only, and a stable failure code. It is assembled from the controller and totals and
/// carries no way to command either.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct HistoricalReport {
    pub schema: String,
    pub state: String,
    pub records_seen: u64,
    pub records_inserted: u64,
    pub duplicates: u64,
    pub rejected_records: u64,
    pub last_cursor_hash: Option<String>,
    pub affected_days: Vec<String>,
    pub failure_code: Option<u16>,
}

impl HistoricalReport {
    pub fn assemble(controller: &HistoricalController, totals: &SyncTotals) -> Self {
        Self {
            schema: HISTORICAL_STATUS_SCHEMA.to_owned(),
            state: controller.state().name().to_owned(),
            records_seen: controller.records_seen(),
            records_inserted: totals.inserted,
            duplicates: totals.duplicates,
            rejected_records: totals.rejected_records,
            last_cursor_hash: controller.last_cursor_hash().map(str::to_owned),
            affected_days: totals.days.iso(),
            failure_code: controller.failure_code(),
        }
    }

    /// The report of a runtime with no historical sync started.
    pub fn idle() -> Self {
        Self {
            schema: HISTORICAL_STATUS_SCHEMA.to_owned(),
            state: HistoricalState::Idle.name().to_owned(),
            records_seen: 0,
            records_inserted: 0,
            duplicates: 0,
            rejected_records: 0,
            last_cursor_hash: None,
            affected_days: Vec::new(),
            failure_code: None,
        }
    }

    pub fn canonical_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| {
            MavError::new(
                codes::STORAGE_SERIALIZE,
                "could not serialise the historical report",
            )
            .context(e.to_string())
        })
    }

    pub fn canonical_hash(&self) -> Result<String> {
        Ok(crate::snapshot::fnv1a_64(self.canonical_json()?.as_bytes()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AwaitedResponse {
    Range,
    Send,
}

struct Outstanding {
    kind: AwaitedResponse,
    command: Command,
    retries: u8,
}

struct PendingBurst {
    ack_payload: Vec<u8>,
    record_count: u32,
}

pub struct HistoricalController {
    config: HistoricalConfig,
    state: HistoricalState,
    next_seq: u8,
    outstanding: Option<Outstanding>,
    burst_open: bool,
    pending_burst: Option<PendingBurst>,
    records_seen: u64,
    records_persisted: u64,
    last_cursor_hash: Option<String>,
    failure_code: Option<u16>,
    ids: Ids,
}

impl HistoricalController {
    pub fn new(config: HistoricalConfig) -> Self {
        Self {
            config,
            state: HistoricalState::Idle,
            next_seq: 1,
            outstanding: None,
            burst_open: false,
            pending_burst: None,
            records_seen: 0,
            records_persisted: 0,
            last_cursor_hash: None,
            failure_code: None,
            ids: Ids::default(),
        }
    }

    pub fn state(&self) -> HistoricalState {
        self.state
    }

    pub fn records_seen(&self) -> u64 {
        self.records_seen
    }

    pub fn records_persisted(&self) -> u64 {
        self.records_persisted
    }

    pub fn awaiting_response(&self) -> bool {
        self.outstanding.is_some()
    }

    /// The FNV-1a hash of the last cursor whose burst committed durably. The raw cursor bytes
    /// stay between the controller and the device; only this hash may cross FFI or enter a log.
    pub fn last_cursor_hash(&self) -> Option<&str> {
        self.last_cursor_hash.as_deref()
    }

    /// The stable `MAV-…` code the sync failed with, once the state is `Failed`.
    pub fn failure_code(&self) -> Option<u16> {
        self.failure_code
    }

    pub fn step(&mut self, event: HistoricalEvent, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        match event {
            HistoricalEvent::Start => self.start(tap),
            HistoricalEvent::Response {
                to_opcode,
                origin_seq,
                result,
            } => self.on_response(to_opcode, origin_seq, result, tap),
            HistoricalEvent::BurstStarted => self.on_burst_started(tap),
            HistoricalEvent::BurstEnded {
                ack_payload,
                record_count,
            } => self.on_burst_ended(ack_payload, record_count, tap),
            HistoricalEvent::BurstPersisted => self.on_burst_persisted(tap),
            HistoricalEvent::PersistFailed { error } => self.on_persist_failed(error, tap),
            HistoricalEvent::HistoryComplete => self.on_history_complete(tap),
            HistoricalEvent::Timeout => self.on_timeout(tap),
            HistoricalEvent::Disconnect => {
                self.outstanding = None;
                self.pending_burst = None;
                self.burst_open = false;
                self.failure_code = Some(codes::TRANSPORT_NATIVE_FAILURE);
                self.transition(HistoricalState::Failed, tap);
                Ok(HistoricalOutcome::default())
            }
        }
    }

    fn start(&mut self, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        if self.state != HistoricalState::Idle {
            return self.fail("historical sync can start only from idle", tap);
        }
        let command = self.issue(AwaitedResponse::Range);
        self.transition(HistoricalState::AwaitingRange, tap);
        Ok(HistoricalOutcome {
            commands: vec![command],
        })
    }

    fn on_response(
        &mut self,
        to_opcode: u8,
        origin_seq: u8,
        result: ResponseResult,
        tap: &dyn Tap,
    ) -> Result<HistoricalOutcome> {
        let Some(outstanding) = self.outstanding.as_ref() else {
            self.reject_unmatched_response(to_opcode, origin_seq, tap);
            return Ok(HistoricalOutcome::default());
        };
        if outstanding.command.opcode != to_opcode || outstanding.command.seq != origin_seq {
            self.reject_unmatched_response(to_opcode, origin_seq, tap);
            return Ok(HistoricalOutcome::default());
        }

        match (outstanding.kind, result) {
            (AwaitedResponse::Range, ResponseResult::Ok) => {
                self.outstanding = None;
                let command = self.issue(AwaitedResponse::Send);
                self.transition(HistoricalState::AwaitingSendAcceptance, tap);
                Ok(HistoricalOutcome {
                    commands: vec![command],
                })
            }
            (AwaitedResponse::Send, ResponseResult::Pending) => Ok(HistoricalOutcome::default()),
            (AwaitedResponse::Send, ResponseResult::Ok) => {
                self.outstanding = None;
                self.transition(HistoricalState::Receiving, tap);
                Ok(HistoricalOutcome::default())
            }
            (_, ResponseResult::Unknown(value)) => self.fail_with(
                MavError::new(
                    codes::TRANSPORT_COMMAND_REJECTED,
                    "historical command returned an unknown result",
                )
                .context(format!("opcode {to_opcode}, result {value}")),
                tap,
            ),
            (AwaitedResponse::Range, ResponseResult::Pending) => self.fail_with(
                MavError::new(
                    codes::TRANSPORT_COMMAND_REJECTED,
                    "data-range request returned pending instead of a range",
                )
                .context(format!("opcode {to_opcode}")),
                tap,
            ),
        }
    }

    fn on_burst_started(&mut self, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        if self.state != HistoricalState::Receiving || self.burst_open {
            return self.fail("history burst started in an invalid state", tap);
        }
        self.burst_open = true;
        Ok(HistoricalOutcome::default())
    }

    fn on_burst_ended(
        &mut self,
        ack_payload: Vec<u8>,
        record_count: u32,
        tap: &dyn Tap,
    ) -> Result<HistoricalOutcome> {
        if self.state != HistoricalState::Receiving || !self.burst_open {
            return self.fail("history burst ended without a matching start", tap);
        }
        if ack_payload.is_empty() || ack_payload.len() > self.config.max_ack_payload_bytes {
            return self.fail_with(
                MavError::new(
                    codes::TRANSPORT_HISTORICAL_PROTOCOL,
                    "historical acknowledgement payload has an invalid length",
                )
                .context(format!(
                    "{} bytes, maximum {}",
                    ack_payload.len(),
                    self.config.max_ack_payload_bytes
                )),
                tap,
            );
        }
        self.burst_open = false;
        self.records_seen = self.records_seen.saturating_add(u64::from(record_count));
        self.pending_burst = Some(PendingBurst {
            ack_payload,
            record_count,
        });
        self.transition(HistoricalState::AwaitingDurableCommit, tap);
        Ok(HistoricalOutcome::default())
    }

    fn on_burst_persisted(&mut self, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        if self.state != HistoricalState::AwaitingDurableCommit {
            return self.fail(
                "burst persistence arrived with no burst awaiting commit",
                tap,
            );
        }
        let Some(pending) = self.pending_burst.take() else {
            return self.fail("durable-commit state has no pending burst", tap);
        };
        self.records_persisted = self
            .records_persisted
            .saturating_add(u64::from(pending.record_count));
        self.last_cursor_hash = Some(crate::snapshot::fnv1a_64(&pending.ack_payload));
        let command = Command {
            opcode: self.config.acknowledge.opcode,
            seq: self.take_seq(),
            b3: self.config.acknowledge.b3,
            payload: pending.ack_payload,
        };
        self.transition(HistoricalState::Receiving, tap);
        Ok(HistoricalOutcome {
            commands: vec![command],
        })
    }

    fn on_persist_failed(&mut self, error: MavError, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        if self.state != HistoricalState::AwaitingDurableCommit {
            return self.fail("persistence failed with no burst awaiting commit", tap);
        }
        self.pending_burst = None;
        self.failure_code = Some(error.code);
        self.transition(HistoricalState::Failed, tap);
        tap.on_stage(
            Stage::Acquisition,
            TapEvent::Rejected {
                error: error.clone(),
                ids: self.ids,
            },
        );
        Err(error)
    }

    fn on_history_complete(&mut self, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        if self.state != HistoricalState::Receiving
            || self.burst_open
            || self.pending_burst.is_some()
        {
            return self.fail(
                "history completed while a burst was open or uncommitted",
                tap,
            );
        }
        self.transition(HistoricalState::Complete, tap);
        Ok(HistoricalOutcome::default())
    }

    fn on_timeout(&mut self, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        let Some(outstanding) = self.outstanding.as_mut() else {
            return Ok(HistoricalOutcome::default());
        };
        if outstanding.retries < self.config.max_retries {
            outstanding.retries += 1;
            return Ok(HistoricalOutcome {
                commands: vec![outstanding.command.clone()],
            });
        }
        let opcode = outstanding.command.opcode;
        self.outstanding = None;
        self.fail_with(
            MavError::new(
                codes::TRANSPORT_COMMAND_TIMEOUT,
                "historical command exhausted its retry budget",
            )
            .context(format!(
                "opcode {opcode}, retries {}",
                self.config.max_retries
            )),
            tap,
        )
    }

    fn issue(&mut self, kind: AwaitedResponse) -> Command {
        let template = match kind {
            AwaitedResponse::Range => self.config.get_data_range.clone(),
            AwaitedResponse::Send => self.config.send_historical.clone(),
        };
        let command = template.command(self.take_seq());
        self.outstanding = Some(Outstanding {
            kind,
            command: command.clone(),
            retries: 0,
        });
        command
    }

    fn take_seq(&mut self) -> u8 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        seq
    }

    fn reject_unmatched_response(&self, to_opcode: u8, origin_seq: u8, tap: &dyn Tap) {
        tap.on_stage(
            Stage::Acquisition,
            TapEvent::Rejected {
                error: MavError::new(
                    codes::TRANSPORT_UNEXPECTED_RESPONSE,
                    "historical response matched no outstanding command",
                )
                .context(format!("to_opcode {to_opcode}, origin_seq {origin_seq}")),
                ids: self.ids,
            },
        );
    }

    fn fail(&mut self, message: &'static str, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        self.fail_with(
            MavError::new(codes::TRANSPORT_HISTORICAL_PROTOCOL, message)
                .context(format!("state {}", self.state.name())),
            tap,
        )
    }

    fn fail_with(&mut self, error: MavError, tap: &dyn Tap) -> Result<HistoricalOutcome> {
        self.outstanding = None;
        self.pending_burst = None;
        self.burst_open = false;
        self.failure_code = Some(error.code);
        self.transition(HistoricalState::Failed, tap);
        tap.on_stage(
            Stage::Acquisition,
            TapEvent::Rejected {
                error: error.clone(),
                ids: self.ids,
            },
        );
        Err(error)
    }

    fn transition(&mut self, to: HistoricalState, tap: &dyn Tap) {
        let from = self.state;
        if from == to {
            return;
        }
        self.state = to;
        tap.on_stage(
            Stage::Acquisition,
            TapEvent::Transition {
                from: from.name(),
                to: to.name(),
                ids: self.ids,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::error::codes;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingTap {
        transitions: Mutex<Vec<(String, String)>>,
        rejections: Mutex<Vec<u16>>,
    }

    impl Tap for RecordingTap {
        fn on_stage(&self, _stage: Stage, event: TapEvent) {
            match event {
                TapEvent::Transition { from, to, .. } => {
                    self.transitions
                        .lock()
                        .unwrap()
                        .push((from.to_owned(), to.to_owned()));
                }
                TapEvent::Rejected { error, .. } => {
                    self.rejections.lock().unwrap().push(error.code);
                }
                TapEvent::Produced { .. } => {}
            }
        }
    }

    fn config() -> HistoricalConfig {
        HistoricalConfig {
            get_data_range: CommandTemplate {
                opcode: 34,
                b3: Some(0),
                payload: Vec::new(),
            },
            send_historical: CommandTemplate {
                opcode: 22,
                b3: Some(0),
                payload: Vec::new(),
            },
            acknowledge: CommandTemplate {
                opcode: 23,
                b3: Some(1),
                payload: Vec::new(),
            },
            max_retries: 3,
            max_ack_payload_bytes: 64,
        }
    }

    fn start(controller: &mut HistoricalController, tap: &RecordingTap) -> Command {
        controller
            .step(HistoricalEvent::Start, tap)
            .unwrap()
            .commands
            .into_iter()
            .next()
            .unwrap()
    }

    fn drive_to_receiving(
        controller: &mut HistoricalController,
        tap: &RecordingTap,
    ) -> Vec<Command> {
        let range = start(controller, tap);
        let send = controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: range.opcode,
                    origin_seq: range.seq,
                    result: ResponseResult::Ok,
                },
                tap,
            )
            .unwrap()
            .commands
            .into_iter()
            .next()
            .unwrap();
        controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: send.opcode,
                    origin_seq: send.seq,
                    result: ResponseResult::Ok,
                },
                tap,
            )
            .unwrap();
        vec![range, send]
    }

    fn end_burst(controller: &mut HistoricalController, tap: &RecordingTap, payload: &[u8]) {
        controller.step(HistoricalEvent::BurstStarted, tap).unwrap();
        controller
            .step(
                HistoricalEvent::BurstEnded {
                    ack_payload: payload.to_vec(),
                    record_count: 7,
                },
                tap,
            )
            .unwrap();
    }

    #[test]
    fn start_requests_the_data_range() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        let command = start(&mut controller, &tap);
        assert_eq!(command.opcode, 34);
        assert_eq!(command.seq, 1);
        assert_eq!(controller.state(), HistoricalState::AwaitingRange);
    }

    #[test]
    fn range_success_requests_history() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        let range = start(&mut controller, &tap);
        let outcome = controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: range.opcode,
                    origin_seq: range.seq,
                    result: ResponseResult::Ok,
                },
                &tap,
            )
            .unwrap();
        assert_eq!(outcome.commands.len(), 1);
        assert_eq!(outcome.commands[0].opcode, 22);
        assert_eq!(outcome.commands[0].seq, 2);
        assert_eq!(controller.state(), HistoricalState::AwaitingSendAcceptance);
    }

    #[test]
    fn pending_send_response_does_not_restart_or_ack() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        let range = start(&mut controller, &tap);
        let send = controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: range.opcode,
                    origin_seq: range.seq,
                    result: ResponseResult::Ok,
                },
                &tap,
            )
            .unwrap()
            .commands
            .remove(0);
        let pending = controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: send.opcode,
                    origin_seq: send.seq,
                    result: ResponseResult::Pending,
                },
                &tap,
            )
            .unwrap();
        assert!(pending.commands.is_empty());
        assert!(controller.awaiting_response());
        assert_eq!(controller.state(), HistoricalState::AwaitingSendAcceptance);

        controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: send.opcode,
                    origin_seq: send.seq,
                    result: ResponseResult::Ok,
                },
                &tap,
            )
            .unwrap();
        assert_eq!(controller.state(), HistoricalState::Receiving);
    }

    #[test]
    fn burst_end_never_acks_before_persistence() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        controller
            .step(HistoricalEvent::BurstStarted, &tap)
            .unwrap();
        let outcome = controller
            .step(
                HistoricalEvent::BurstEnded {
                    ack_payload: vec![1, 2, 3, 4],
                    record_count: 7,
                },
                &tap,
            )
            .unwrap();
        assert!(outcome.commands.is_empty());
        assert_eq!(controller.state(), HistoricalState::AwaitingDurableCommit);
        assert_eq!(controller.records_seen(), 7);
        assert_eq!(controller.records_persisted(), 0);
    }

    #[test]
    fn persisted_burst_acks_exact_payload_once() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        let cursor = vec![0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80];
        end_burst(&mut controller, &tap, &cursor);
        let outcome = controller
            .step(HistoricalEvent::BurstPersisted, &tap)
            .unwrap();
        assert_eq!(outcome.commands.len(), 1);
        assert_eq!(outcome.commands[0].opcode, 23);
        assert_eq!(outcome.commands[0].seq, 3);
        assert_eq!(outcome.commands[0].payload, cursor);
        assert_eq!(controller.records_persisted(), 7);
        assert_eq!(controller.state(), HistoricalState::Receiving);
    }

    #[test]
    fn persist_failure_permanently_blocks_the_ack() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        end_burst(&mut controller, &tap, &[1, 2, 3]);
        let storage_error = MavError::new(codes::STORAGE_QUERY, "injected commit failure");
        let error = controller
            .step(
                HistoricalEvent::PersistFailed {
                    error: storage_error,
                },
                &tap,
            )
            .unwrap_err();
        assert_eq!(error.code, codes::STORAGE_QUERY);
        assert_eq!(controller.state(), HistoricalState::Failed);
        assert_eq!(controller.records_persisted(), 0);
        assert!(controller
            .step(HistoricalEvent::BurstPersisted, &tap)
            .is_err());
    }

    #[test]
    fn history_complete_is_rejected_while_a_burst_is_uncommitted() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        end_burst(&mut controller, &tap, &[1, 2, 3]);
        let error = controller
            .step(HistoricalEvent::HistoryComplete, &tap)
            .unwrap_err();
        assert_eq!(error.code, codes::TRANSPORT_HISTORICAL_PROTOCOL);
        assert_eq!(controller.state(), HistoricalState::Failed);
    }

    #[test]
    fn wrong_sequence_does_not_advance_the_controller() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        let range = start(&mut controller, &tap);
        let outcome = controller
            .step(
                HistoricalEvent::Response {
                    to_opcode: range.opcode,
                    origin_seq: range.seq.wrapping_add(1),
                    result: ResponseResult::Ok,
                },
                &tap,
            )
            .unwrap();
        assert!(outcome.commands.is_empty());
        assert_eq!(controller.state(), HistoricalState::AwaitingRange);
        assert!(controller.awaiting_response());
        assert_eq!(
            *tap.rejections.lock().unwrap(),
            vec![codes::TRANSPORT_UNEXPECTED_RESPONSE]
        );
    }

    #[test]
    fn timeouts_retry_to_the_exact_budget_then_fail_closed() {
        let mut cfg = config();
        cfg.max_retries = 2;
        let mut controller = HistoricalController::new(cfg);
        let tap = RecordingTap::default();
        let range = start(&mut controller, &tap);
        for _ in 0..2 {
            let outcome = controller.step(HistoricalEvent::Timeout, &tap).unwrap();
            assert_eq!(outcome.commands, vec![range.clone()]);
        }
        let error = controller.step(HistoricalEvent::Timeout, &tap).unwrap_err();
        assert_eq!(error.code, codes::TRANSPORT_COMMAND_TIMEOUT);
        assert_eq!(controller.state(), HistoricalState::Failed);
        assert!(!controller.awaiting_response());
    }

    #[test]
    fn disconnect_with_uncommitted_data_emits_no_command() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        end_burst(&mut controller, &tap, &[1, 2, 3]);
        let outcome = controller.step(HistoricalEvent::Disconnect, &tap).unwrap();
        assert!(outcome.commands.is_empty());
        assert_eq!(controller.state(), HistoricalState::Failed);
        assert_eq!(controller.records_persisted(), 0);
    }

    // M5-P7: the progress read model. Hosts render it; they cannot reproduce or command the
    // controller through it.

    fn receipt(inserted: u32, duplicates: u32, day_index: i64) -> crate::burst::BurstReceipt {
        let mut affected_days = crate::recompute::AffectedDays::default();
        if inserted > 0 {
            affected_days.insert(crate::recompute::LocalDay::from_index(day_index));
        }
        crate::burst::BurstReceipt {
            inserted,
            duplicates,
            affected_days,
        }
    }

    #[test]
    fn the_status_report_serializes_canonically_with_a_stable_hash() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        end_burst(
            &mut controller,
            &tap,
            &[0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80],
        );
        controller
            .step(HistoricalEvent::BurstPersisted, &tap)
            .unwrap();

        let mut totals = SyncTotals::default();
        totals.absorb(&receipt(2, 1, 20_285));
        totals.note_rejected(1);

        let report = HistoricalReport::assemble(&controller, &totals);
        assert_eq!(
            report.canonical_json().unwrap(),
            "{\"schema\":\"historical-status/v1\",\"state\":\"historical_receiving\",\
             \"records_seen\":7,\"records_inserted\":2,\"duplicates\":1,\"rejected_records\":1,\
             \"last_cursor_hash\":\"763b696d4565b5c5\",\"affected_days\":[\"2025-07-16\"],\
             \"failure_code\":null}"
        );
        assert_eq!(report.canonical_hash().unwrap(), {
            let again = HistoricalReport::assemble(&controller, &totals);
            again.canonical_hash().unwrap()
        });
    }

    #[test]
    fn the_cursor_crosses_only_as_a_hash() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        end_burst(&mut controller, &tap, &[0xDE, 0xAD, 0xBE, 0xEF]);
        controller
            .step(HistoricalEvent::BurstPersisted, &tap)
            .unwrap();

        let report = HistoricalReport::assemble(&controller, &SyncTotals::default());
        assert_eq!(report.last_cursor_hash.as_deref(), Some("277045760cdd0993"));
        let json = report.canonical_json().unwrap().to_lowercase();
        assert!(!json.contains("deadbeef"));
        assert!(!json.contains("222,173"));
    }

    #[test]
    fn a_persistence_failure_reports_a_stable_code_and_blocks_the_ack() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        end_burst(&mut controller, &tap, &[1, 2, 3]);
        controller
            .step(
                HistoricalEvent::PersistFailed {
                    error: MavError::new(codes::STORAGE_QUERY, "injected commit failure"),
                },
                &tap,
            )
            .unwrap_err();

        let report = HistoricalReport::assemble(&controller, &SyncTotals::default());
        assert_eq!(report.state, "historical_failed");
        assert_eq!(report.failure_code, Some(codes::STORAGE_QUERY));
        assert_eq!(report.last_cursor_hash, None);
        assert!(controller
            .step(HistoricalEvent::BurstPersisted, &tap)
            .is_err());
    }

    #[test]
    fn a_disconnect_reports_the_transport_failure_code() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        drive_to_receiving(&mut controller, &tap);
        controller.step(HistoricalEvent::Disconnect, &tap).unwrap();
        let report = HistoricalReport::assemble(&controller, &SyncTotals::default());
        assert_eq!(report.state, "historical_failed");
        assert_eq!(report.failure_code, Some(codes::TRANSPORT_NATIVE_FAILURE));
    }

    #[test]
    fn controller_has_no_force_trim_action() {
        let mut controller = HistoricalController::new(config());
        let tap = RecordingTap::default();
        let mut commands = drive_to_receiving(&mut controller, &tap);
        end_burst(&mut controller, &tap, &[1, 2, 3]);
        commands.extend(
            controller
                .step(HistoricalEvent::BurstPersisted, &tap)
                .unwrap()
                .commands,
        );
        controller
            .step(HistoricalEvent::HistoryComplete, &tap)
            .unwrap();
        assert_eq!(
            commands
                .iter()
                .map(|command| command.opcode)
                .collect::<Vec<_>>(),
            vec![34, 22, 23]
        );
        assert!(commands.iter().all(|command| command.opcode != 25));
        assert_eq!(controller.state(), HistoricalState::Complete);
    }
}
