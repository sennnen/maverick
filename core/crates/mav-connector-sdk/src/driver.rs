use crate::abi::{ActionBatch, ConnectorEvent, Validate};
use crate::{Connector, ConnectorError};

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

    pub fn into_inner(self) -> C {
        self.connector
    }
}
