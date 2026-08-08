//! Shared fixture and transport mechanics for ry integration tests.
//!
//! This crate deliberately knows nothing about the checker, CLI, or LSP.
//! Owning crates adapt their production interfaces to [`Driver`].

mod fixture;
mod json_rpc;
mod observed;
mod process;

pub use fixture::FixtureProject;
pub use json_rpc::{AsyncJsonRpcClient, JsonRpcProcess};
pub use observed::{
    Driver, DriverError, ObservedDiagnostic, ObservedFix, ObservedPosition, ObservedRange,
    PositionEncoding, normalize_path, normalize_position,
};
pub use process::CliProcess;
