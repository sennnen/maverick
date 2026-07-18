use crate::{Artifact, ConnectorInstance, LimitProfile};
use mav_connector_abi::{ActionBatch, EventBody, FixtureCase};
use mav_model::error::{codes, MavError, Result};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureResult {
    pub name: String,
    pub events_run: u32,
    pub final_state_hash: [u8; 32],
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
    let first = &fixture.events[0];
    let mut next = 0_usize;
    if fixture.initial_state.is_empty() {
        compare(
            &fixture.name,
            0,
            &instance.init(first)?,
            &fixture.expected[0],
        )?;
        next = 1;
    } else {
        let mut restore = first.clone();
        restore.body = EventBody::RestoreState {
            bytes: fixture.initial_state.clone(),
        };
        compare(
            &fixture.name,
            0,
            &instance.init(&restore)?,
            &ActionBatch {
                actions: Vec::new(),
            },
        )?;
    }
    for index in next..fixture.events.len() {
        compare(
            &fixture.name,
            index,
            &instance.handle(&fixture.events[index])?,
            &fixture.expected[index],
        )?;
    }
    let state = instance.snapshot()?;
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
        final_state_hash,
    })
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
