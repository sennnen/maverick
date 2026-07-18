//! Durable installation, activation, trust, and connector-scoped state.
#![forbid(unsafe_code)]

mod model;
mod repository;

pub use model::{
    ApprovalToken, ConnectorSource, InspectionApproval, InstallRequest, InstalledConnector,
    RemovalMode, SourceKind, StateNamespace, StoredState,
};
pub use repository::ConnectorRepository;
