use crate::abi::{ActionBatch, ConnectorEvent, Validate};
use crate::{Connector, ConnectorError};
use sha2::Digest;

pub struct TestDriver<C> {
    connector: C,
}

impl<C: Connector> TestDriver<C> {
    pub fn new(connector: C) -> Self {
        Self { connector }
    }

    pub fn init(&mut self, event: ConnectorEvent) -> Result<ActionBatch, ConnectorError> {
        event.validate()?;
        let batch = self.connector.init(event)?;
        batch.validate()?;
        Ok(batch)
    }

    pub fn drive(&mut self, event: ConnectorEvent) -> Result<ActionBatch, ConnectorError> {
        event.validate()?;
        let batch = self.connector.handle(event)?;
        batch.validate()?;
        Ok(batch)
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, ConnectorError> {
        self.connector.snapshot()
    }

    /// The digest a fixture case pins its expected end state by. Here rather than in each
    /// connector, so a connector crate needs no hashing dependency of its own and every fixture
    /// across every publisher is hashed exactly one way.
    pub fn snapshot_hash(&self) -> Result<[u8; 32], ConnectorError> {
        Ok(sha2::Sha256::digest(self.snapshot()?).into())
    }

    pub fn into_inner(self) -> C {
        self.connector
    }
}
