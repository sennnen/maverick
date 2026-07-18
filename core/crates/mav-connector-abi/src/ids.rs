use crate::{bounds, Validate, WireError};
use minicbor::{Decode, Encode};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
#[cbor(transparent)]
pub struct ConnectorId(String);

impl ConnectorId {
    pub fn new(value: impl Into<String>) -> Result<Self, WireError> {
        let value = value.into();
        bounds::text(&value, bounds::MAX_CONNECTOR_ID_BYTES, "connector id")?;
        let segments: Vec<&str> = value.split('.').collect();
        if segments.len() < 2
            || segments
                .iter()
                .any(|segment| bounds::identifier(segment, 63, "connector id").is_err())
        {
            return Err(WireError::Schema("connector id"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Validate for ConnectorId {
    fn validate(&self) -> Result<(), WireError> {
        Self::new(self.0.clone()).map(|_| ())
    }
}

macro_rules! numeric_id {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
        #[cbor(transparent)]
        pub struct $name(pub $inner);

        impl Validate for $name {
            fn validate(&self) -> Result<(), WireError> {
                Ok(())
            }
        }
    };
}

numeric_id!(SessionId, u64);
numeric_id!(EventSequence, u64);
numeric_id!(CancellationGeneration, u64);
numeric_id!(OperationId, u64);
numeric_id!(TimerToken, u64);
numeric_id!(BatchId, u64);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
#[cbor(transparent)]
pub struct LimitsProfileId(String);

impl LimitsProfileId {
    pub fn new(value: impl Into<String>) -> Result<Self, WireError> {
        let value = value.into();
        bounds::identifier(&value, bounds::MAX_LOGICAL_ID_BYTES, "limits profile id")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Validate for LimitsProfileId {
    fn validate(&self) -> Result<(), WireError> {
        Self::new(self.0.clone()).map(|_| ())
    }
}
