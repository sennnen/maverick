//! The acquisition stage: the connection state machine and the command/response layer.
//!
//! It is driven entirely by injected events and returns the commands the transport should write
//! and the frames it reassembled, so it holds no radio and no clock. Time enters only as an
//! injected `Timeout` event, which is what makes the retry logic testable to the exact count. The
//! surveyed codebases could never CI-test their BLE state machines because those machines were
//! welded to the platform Bluetooth stack; this one is welded to nothing.
//!
//! The command opcodes come in through `HandshakeConfig` rather than from a manifest, so this crate
//! stays independent of `mav-codec`; the engine fills the config from the device's manifest.

use mav_frame::frame::{RawFrame, WireFormat};
use mav_frame::reassembler::{Reassembler, ReassemblyEvent};
use mav_model::error::{codes, MavError, Result};
use mav_obs::stage::Stage;
use mav_obs::tap::{Ids, Tap, TapEvent};

/// The connection states, in the order a healthy session moves through them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Disconnected,
    Scanning,
    Connecting,
    Authenticating,
    Configuring,
    Streaming,
    HistoricalSync,
    Idle,
}

impl State {
    pub const fn name(self) -> &'static str {
        match self {
            State::Disconnected => "disconnected",
            State::Scanning => "scanning",
            State::Connecting => "connecting",
            State::Authenticating => "authenticating",
            State::Configuring => "configuring",
            State::Streaming => "streaming",
            State::HistoricalSync => "historical_sync",
            State::Idle => "idle",
        }
    }
}

/// Events injected from the transport (or a test, or `mav-replay`). Nothing here reads a clock or a
/// radio; a `Timeout` is the transport telling the machine that a response is overdue.
#[derive(Clone, PartialEq, Debug)]
pub enum Event {
    /// Begin scanning for the device.
    StartScan,
    /// The device advertised and was selected.
    PeripheralDiscovered,
    /// The BLE connection is up (and, for a bonded gen5 strap, the OS bond is in place).
    Connected,
    /// A command response arrived, identifying the command it answers and its sequence number.
    Response { to_opcode: u8, origin_seq: u8 },
    /// Raw notification bytes arrived on the data characteristic.
    Bytes(Vec<u8>),
    /// The outstanding command's response is overdue.
    Timeout,
    /// A historical backfill began.
    HistoricalSyncStarted,
    /// The historical backfill finished.
    HistoricalSyncComplete,
    /// The link dropped, from any state.
    Disconnect,
}

/// A command the machine wants written to the strap. Turning it into frame bytes is the transport's
/// job (`mav_frame::build_frame` plus the inner layout); this type carries the fields.
#[derive(Clone, PartialEq, Debug)]
pub struct Command {
    pub opcode: u8,
    pub seq: u8,
    pub b3: Option<u8>,
    pub payload: Vec<u8>,
}

/// The static command facts the handshake needs, filled from a device manifest by the engine.
#[derive(Clone, PartialEq, Debug)]
pub struct HandshakeConfig {
    pub wire_format: WireFormat,
    pub hello_opcode: u8,
    pub hello_b3: Option<u8>,
    pub hello_payload: Vec<u8>,
    pub realtime_opcode: u8,
    pub realtime_b3: Option<u8>,
    /// How many times a command is resent before the machine gives up. The backoff timing between
    /// resends is the transport's concern; the machine only counts.
    pub max_retries: u8,
}

/// What a single `step` produced: commands to write and frames reassembled from incoming bytes.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct StepOutcome {
    pub commands: Vec<Command>,
    pub frames: Vec<RawFrame>,
}

struct Outstanding {
    command: Command,
    retries: u8,
}

/// The acquisition state machine. Hold one per connection; feed it events and write what it hands
/// back.
pub struct Acquisition {
    config: HandshakeConfig,
    state: State,
    next_seq: u8,
    outstanding: Option<Outstanding>,
    reassembler: Reassembler,
    ids: Ids,
}

impl Acquisition {
    pub fn new(config: HandshakeConfig) -> Self {
        let reassembler = Reassembler::new(config.wire_format);
        Self {
            config,
            state: State::Disconnected,
            next_seq: 1,
            outstanding: None,
            reassembler,
            ids: Ids::default(),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// True while a command is awaiting its response.
    pub fn awaiting_response(&self) -> bool {
        self.outstanding.is_some()
    }

    /// Advance the machine by one event. Returns the commands to write and any frames reassembled.
    /// A `Timeout` that exhausts the retry budget returns a `Transport` error; the caller logs it
    /// and the machine is left disconnected.
    pub fn step(&mut self, event: Event, tap: &dyn Tap) -> Result<StepOutcome> {
        let mut outcome = StepOutcome::default();
        match event {
            Event::StartScan => self.transition(State::Scanning, tap),
            Event::PeripheralDiscovered => {
                if self.state == State::Scanning {
                    self.transition(State::Connecting, tap);
                }
            }
            Event::Connected => {
                self.transition(State::Authenticating, tap);
                outcome.commands.push(self.send_hello(tap));
            }
            Event::Response {
                to_opcode,
                origin_seq,
            } => {
                self.on_response(to_opcode, origin_seq, tap, &mut outcome)?;
            }
            Event::Timeout => {
                self.on_timeout(tap, &mut outcome)?;
            }
            Event::Bytes(bytes) => self.on_bytes(&bytes, tap, &mut outcome),
            Event::HistoricalSyncStarted => self.transition(State::HistoricalSync, tap),
            Event::HistoricalSyncComplete => {
                if self.state == State::HistoricalSync {
                    self.transition(State::Idle, tap);
                }
            }
            Event::Disconnect => {
                self.outstanding = None;
                self.reassembler.reset();
                self.transition(State::Disconnected, tap);
            }
        }
        Ok(outcome)
    }

    fn on_response(
        &mut self,
        to_opcode: u8,
        origin_seq: u8,
        tap: &dyn Tap,
        outcome: &mut StepOutcome,
    ) -> Result<()> {
        let matched = self
            .outstanding
            .as_ref()
            .is_some_and(|o| o.command.opcode == to_opcode && o.command.seq == origin_seq);
        if !matched {
            let error = MavError::new(
                codes::TRANSPORT_UNEXPECTED_RESPONSE,
                "response matched no outstanding command",
            )
            .context(format!("to_opcode {to_opcode}, origin_seq {origin_seq}"));
            tap.on_stage(
                Stage::Acquisition,
                TapEvent::Rejected {
                    error,
                    ids: self.ids,
                },
            );
            return Ok(());
        }
        self.outstanding = None;
        match self.state {
            State::Authenticating => {
                self.transition(State::Configuring, tap);
                outcome.commands.push(self.send_realtime(tap));
            }
            State::Configuring => self.transition(State::Streaming, tap),
            _ => {}
        }
        Ok(())
    }

    fn on_timeout(&mut self, tap: &dyn Tap, outcome: &mut StepOutcome) -> Result<()> {
        let Some(outstanding) = self.outstanding.as_mut() else {
            return Ok(());
        };
        if outstanding.retries < self.config.max_retries {
            outstanding.retries += 1;
            let resend = outstanding.command.clone();
            outcome.commands.push(resend);
            Ok(())
        } else {
            let opcode = outstanding.command.opcode;
            self.outstanding = None;
            self.transition(State::Disconnected, tap);
            Err(MavError::new(
                codes::TRANSPORT_COMMAND_TIMEOUT,
                "command response never arrived within the retry budget",
            )
            .context(format!(
                "opcode {opcode}, retries {}",
                self.config.max_retries
            )))
        }
    }

    fn on_bytes(&mut self, bytes: &[u8], tap: &dyn Tap, outcome: &mut StepOutcome) {
        if !matches!(self.state, State::Streaming | State::HistoricalSync) {
            let error = MavError::warning(
                codes::TRANSPORT_UNEXPECTED_BYTES,
                "data bytes arrived outside a streaming state",
            )
            .context(format!(
                "state {}, {} bytes",
                self.state.name(),
                bytes.len()
            ));
            tap.on_stage(
                Stage::Acquisition,
                TapEvent::Rejected {
                    error,
                    ids: self.ids,
                },
            );
            return;
        }
        for event in self.reassembler.push(bytes) {
            match event {
                ReassemblyEvent::Frame(frame) => outcome.frames.push(frame),
                ReassemblyEvent::InvalidFrame(error) => {
                    tap.on_stage(
                        Stage::Acquisition,
                        TapEvent::Rejected {
                            error,
                            ids: self.ids,
                        },
                    );
                }
                ReassemblyEvent::SkippedGarbage { bytes } => {
                    let error = MavError::warning(
                        codes::FRAME_GARBAGE_SKIPPED,
                        "bytes discarded while resynchronising",
                    )
                    .context(format!("{bytes} bytes"));
                    tap.on_stage(
                        Stage::Acquisition,
                        TapEvent::Rejected {
                            error,
                            ids: self.ids,
                        },
                    );
                }
            }
        }
        if !outcome.frames.is_empty() {
            tap.on_stage(
                Stage::Acquisition,
                TapEvent::Produced {
                    count: outcome.frames.len(),
                    ids: self.ids,
                    summary: None,
                },
            );
        }
    }

    fn send_hello(&mut self, _tap: &dyn Tap) -> Command {
        let command = Command {
            opcode: self.config.hello_opcode,
            seq: self.take_seq(),
            b3: self.config.hello_b3,
            payload: self.config.hello_payload.clone(),
        };
        self.outstanding = Some(Outstanding {
            command: command.clone(),
            retries: 0,
        });
        command
    }

    fn send_realtime(&mut self, _tap: &dyn Tap) -> Command {
        let command = Command {
            opcode: self.config.realtime_opcode,
            seq: self.take_seq(),
            b3: self.config.realtime_b3,
            payload: Vec::new(),
        };
        self.outstanding = Some(Outstanding {
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

    fn transition(&mut self, to: State, tap: &dyn Tap) {
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
    use mav_frame::frame::build_frame;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingTap {
        transitions: Mutex<Vec<(String, String)>>,
        rejections: Mutex<Vec<u16>>,
        produced: Mutex<usize>,
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
                TapEvent::Produced { count, .. } => *self.produced.lock().unwrap() += count,
            }
        }
    }

    fn machine() -> (Acquisition, RecordingTap) {
        (
            Acquisition::new(HandshakeConfig {
                wire_format: WireFormat::Gen5,
                hello_opcode: 145,
                hello_b3: Some(1),
                hello_payload: vec![0x01],
                realtime_opcode: 3,
                realtime_b3: None,
                max_retries: 3,
            }),
            RecordingTap::default(),
        )
    }

    #[test]
    fn handshake_reaches_streaming() {
        let (mut m, tap) = machine();

        let out = m.step(Event::StartScan, &tap).unwrap();
        assert_eq!(m.state(), State::Scanning);
        assert!(out.commands.is_empty());

        m.step(Event::PeripheralDiscovered, &tap).unwrap();
        assert_eq!(m.state(), State::Connecting);

        let out = m.step(Event::Connected, &tap).unwrap();
        assert_eq!(m.state(), State::Authenticating);
        assert_eq!(out.commands.len(), 1, "connecting sends the hello");
        assert_eq!(out.commands[0].opcode, 145);
        assert_eq!(out.commands[0].payload, vec![0x01]);
        let hello_seq = out.commands[0].seq;

        let out = m
            .step(
                Event::Response {
                    to_opcode: 145,
                    origin_seq: hello_seq,
                },
                &tap,
            )
            .unwrap();
        assert_eq!(m.state(), State::Configuring);
        assert_eq!(
            out.commands.len(),
            1,
            "the hello ack sends the realtime toggle"
        );
        assert_eq!(out.commands[0].opcode, 3);
        let realtime_seq = out.commands[0].seq;

        m.step(
            Event::Response {
                to_opcode: 3,
                origin_seq: realtime_seq,
            },
            &tap,
        )
        .unwrap();
        assert_eq!(m.state(), State::Streaming);
        assert!(!m.awaiting_response());

        assert_eq!(
            *tap.transitions.lock().unwrap(),
            vec![
                ("disconnected".into(), "scanning".into()),
                ("scanning".into(), "connecting".into()),
                ("connecting".into(), "authenticating".into()),
                ("authenticating".into(), "configuring".into()),
                ("configuring".into(), "streaming".into()),
            ]
        );
    }

    #[test]
    fn command_retries_then_fails() {
        let (mut m, tap) = machine();
        m.step(Event::Connected, &tap).unwrap();
        assert!(m.awaiting_response());

        // Three timeouts each resend the hello; the fourth exhausts the budget.
        for _ in 0..3 {
            let out = m.step(Event::Timeout, &tap).unwrap();
            assert_eq!(out.commands.len(), 1, "a timeout under budget resends");
            assert_eq!(out.commands[0].opcode, 145);
        }
        let err = m.step(Event::Timeout, &tap).err().unwrap();
        assert_eq!(err.code, codes::TRANSPORT_COMMAND_TIMEOUT);
        assert_eq!(m.state(), State::Disconnected);
        assert!(!m.awaiting_response());
    }

    #[test]
    fn sequence_mismatch_is_rejected() {
        let (mut m, tap) = machine();
        let out = m.step(Event::Connected, &tap).unwrap();
        let hello_seq = out.commands[0].seq;

        // A response with the wrong sequence must not match the outstanding hello.
        let out = m
            .step(
                Event::Response {
                    to_opcode: 145,
                    origin_seq: hello_seq.wrapping_add(1),
                },
                &tap,
            )
            .unwrap();
        assert!(out.commands.is_empty());
        assert_eq!(m.state(), State::Authenticating, "state is unchanged");
        assert!(m.awaiting_response(), "the command is still outstanding");
        assert_eq!(
            *tap.rejections.lock().unwrap(),
            vec![codes::TRANSPORT_UNEXPECTED_RESPONSE]
        );
    }

    #[test]
    fn disconnect_transitions_to_disconnected_from_any_state() {
        for reach in [State::Streaming, State::Configuring, State::HistoricalSync] {
            let (mut m, tap) = machine();
            drive_to(&mut m, reach, &tap);
            assert_eq!(m.state(), reach);

            m.step(Event::Disconnect, &tap).unwrap();
            assert_eq!(m.state(), State::Disconnected);
            let transitions = tap.transitions.lock().unwrap();
            assert_eq!(
                transitions.last().unwrap(),
                &(reach.name().to_owned(), "disconnected".to_owned())
            );
        }
    }

    #[test]
    fn streaming_bytes_reassemble_into_frames() {
        let (mut m, tap) = machine();
        drive_to(&mut m, State::Streaming, &tap);

        let wire = build_frame(WireFormat::Gen5, &[0x28, 0x01, 0x00, 0x42]).unwrap();
        let out = m.step(Event::Bytes(wire), &tap).unwrap();
        assert_eq!(out.frames.len(), 1);
        assert_eq!(out.frames[0].payload, vec![0x28, 0x01, 0x00, 0x42]);
        assert_eq!(*tap.produced.lock().unwrap(), 1);
    }

    #[test]
    fn bytes_outside_streaming_are_rejected_not_reassembled() {
        let (mut m, tap) = machine();
        let out = m.step(Event::Bytes(vec![0xAA, 0x01]), &tap).unwrap();
        assert!(out.frames.is_empty());
        assert_eq!(
            *tap.rejections.lock().unwrap(),
            vec![codes::TRANSPORT_UNEXPECTED_BYTES]
        );
    }

    /// Walk the machine into a target state through the normal event sequence.
    fn drive_to(m: &mut Acquisition, target: State, tap: &RecordingTap) {
        m.step(Event::Connected, tap).unwrap();
        let hello_seq = 1u8;
        let out = m
            .step(
                Event::Response {
                    to_opcode: 145,
                    origin_seq: hello_seq,
                },
                tap,
            )
            .unwrap();
        if target == State::Configuring {
            return;
        }
        let realtime_seq = out.commands[0].seq;
        m.step(
            Event::Response {
                to_opcode: 3,
                origin_seq: realtime_seq,
            },
            tap,
        )
        .unwrap();
        if target == State::Streaming {
            return;
        }
        if target == State::HistoricalSync {
            m.step(Event::HistoricalSyncStarted, tap).unwrap();
        }
    }
}
