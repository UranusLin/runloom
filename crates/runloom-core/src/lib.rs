//! Local metadata only. This crate does not launch agents or authenticate clients.
mod model;
mod store;
pub use model::{Event, NewWork, Work};
pub use store::{Error, Store};

mod execution;
pub use execution::{CommandSpec, Execution};
pub const SCHEMA_VERSION: u32 = 4;
mod reservation;

mod control;
pub use control::{InputRequest, LeaseGrant, LeaseProof, LeaseStatus, ResizeRequest};

mod provider;
pub use provider::{resume_plan, ProviderCapabilities};
