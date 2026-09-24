//! Shared fixture and transport mechanics for ry integration tests.

mod fixture;
mod json_rpc;
mod lsp_session;
mod observed;
mod process;

pub use fixture::FixtureProject;
pub use json_rpc::{AsyncJsonRpcClient, JsonRpcProcess};
pub use lsp_session::{LspSession, file_uri, rpc_receive_timeout};
pub use observed::{
    DriverError, ObservedPosition, PositionEncoding, normalize_path, normalize_position,
};
pub use process::CliProcess;
