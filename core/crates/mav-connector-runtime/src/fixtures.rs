use crate::{Artifact, ConnectorInstance, LimitProfile};
use mav_connector_abi::{encode_canonical, ActionBatch, ActionBody, EventBody, FixtureCase};
use mav_model::error::{codes, MavError, Result};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureResult {
    pub name: String,
    pub events_run: u32,
    pub input_hash: [u8; 32],
    pub action_trace_hash: [u8; 32],
    pub sample_hash: [u8; 32],
    pub final_state_hash: [u8; 32],
    pub max_fuel_consumed: u64,
    pub peak_memory_bytes: u64,
}

impl Artifact {
    pub fn run_fixtures(&self, profile: LimitProfile) -> Result<Vec<FixtureResult>> {
        let mut results = Vec::with_capacity(self.report().fixtures.cases.len());
        for fixture in &self.report().fixtures.cases {
            results.push(run_fixture(self, fixture, &profile)?);
        }
        Ok(results)
    }
}

fn run_fixture(
    artifact: &Artifact,
    fixture: &FixtureCase,
    profile: &LimitProfile,
) -> Result<FixtureResult> {
    if fixture.events.is_empty()
        || fixture.events.len() != fixture.expected.len()
        || fixture.max_fuel == 0
    {
        return Err(error(
            codes::CONNECTOR_RUNTIME_FIXTURE_INVALID,
            format!(
                "fixture {} has invalid event/action/fuel shape",
                fixture.name
            ),
        ));
    }
    let fixture_profile = profile.for_fixture(fixture.max_fuel);
    let mut instance = ConnectorInstance::instantiate(artifact, fixture_profile)?;
    let mut input_hasher = Sha256::new();
    let mut action_hasher = Sha256::new();
    let mut sample_hasher = Sha256::new();
    let mut max_fuel_consumed = 0_u64;
    let mut peak_memory_bytes = 0_u64;
    let first = &fixture.events[0];
    let mut next = 0_usize;
    if fixture.initial_state.is_empty() {
        let actual = instance.init(first)?;
        compare(&fixture.name, 0, &actual, &fixture.expected[0])?;
        observe(
            &instance,
            first,
            &actual,
            &mut input_hasher,
            &mut action_hasher,
            &mut sample_hasher,
            &mut max_fuel_consumed,
            &mut peak_memory_bytes,
        )?;
        next = 1;
    } else {
        let mut restore = first.clone();
        restore.body = EventBody::RestoreState {
            bytes: fixture.initial_state.clone(),
        };
        let actual = instance.init(&restore)?;
        compare(
            &fixture.name,
            0,
            &actual,
            &ActionBatch {
                actions: Vec::new(),
            },
        )?;
        observe(
            &instance,
            &restore,
            &actual,
            &mut input_hasher,
            &mut action_hasher,
            &mut sample_hasher,
            &mut max_fuel_consumed,
            &mut peak_memory_bytes,
        )?;
    }
    for index in next..fixture.events.len() {
        let actual = instance.handle(&fixture.events[index])?;
        compare(&fixture.name, index, &actual, &fixture.expected[index])?;
        observe(
            &instance,
            &fixture.events[index],
            &actual,
            &mut input_hasher,
            &mut action_hasher,
            &mut sample_hasher,
            &mut max_fuel_consumed,
            &mut peak_memory_bytes,
        )?;
    }
    let state = instance.snapshot()?;
    let (fuel, memory) = instance.resource_usage()?;
    max_fuel_consumed = max_fuel_consumed.max(fuel);
    peak_memory_bytes = peak_memory_bytes.max(memory);
    let final_state_hash: [u8; 32] = Sha256::digest(state).into();
    if final_state_hash != fixture.expected_state_hash {
        return Err(error(
            codes::CONNECTOR_RUNTIME_FIXTURE_MISMATCH,
            format!("fixture {} final state hash differs", fixture.name),
        ));
    }
    Ok(FixtureResult {
        name: fixture.name.clone(),
        events_run: fixture.events.len() as u32,
        input_hash: input_hasher.finalize().into(),
        action_trace_hash: action_hasher.finalize().into(),
        sample_hash: sample_hasher.finalize().into(),
        final_state_hash,
        max_fuel_consumed,
        peak_memory_bytes,
    })
}

#[allow(clippy::too_many_arguments)]
fn observe(
    instance: &ConnectorInstance,
    event: &mav_connector_abi::ConnectorEvent,
    actions: &ActionBatch,
    input_hasher: &mut Sha256,
    action_hasher: &mut Sha256,
    sample_hasher: &mut Sha256,
    max_fuel_consumed: &mut u64,
    peak_memory_bytes: &mut u64,
) -> Result<()> {
    input_hasher.update(encode_canonical(event).map_err(|source| {
        error(
            codes::CONNECTOR_RUNTIME_FIXTURE_INVALID,
            format!("fixture event encoding failed: {source}"),
        )
    })?);
    action_hasher.update(encode_canonical(actions).map_err(|source| {
        error(
            codes::CONNECTOR_RUNTIME_FIXTURE_INVALID,
            format!("fixture action encoding failed: {source}"),
        )
    })?);
    for action in &actions.actions {
        if let ActionBody::EmitSamples { samples, .. } = &action.body {
            for sample in samples {
                sample_hasher.update(encode_canonical(sample).map_err(|source| {
                    error(
                        codes::CONNECTOR_RUNTIME_FIXTURE_INVALID,
                        format!("fixture sample encoding failed: {source}"),
                    )
                })?);
            }
        }
    }
    let (fuel, memory) = instance.resource_usage()?;
    *max_fuel_consumed = (*max_fuel_consumed).max(fuel);
    *peak_memory_bytes = (*peak_memory_bytes).max(memory);
    Ok(())
}

fn compare(name: &str, index: usize, actual: &ActionBatch, expected: &ActionBatch) -> Result<()> {
    if actual != expected {
        return Err(error(
            codes::CONNECTOR_RUNTIME_FIXTURE_MISMATCH,
            format!("fixture {name} action batch {index} differs"),
        ));
    }
    Ok(())
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}
