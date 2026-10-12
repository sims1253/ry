//! End-to-end tests against the real `ry` binary. They live in one test
//! target because each `tests/*.rs` file links its own copy of the whole
//! workspace; add new suites as modules here.

mod box_interop;
mod config_e2e;
mod dump_types_e2e;
mod editor_playground;
mod encoding_e2e;
mod facts_context;
mod facts_e2e;
mod facts_references;
mod lsp_testkit;
mod report_inputs;
mod stdin_e2e;
mod suppression_e2e;
mod testkit;
mod typehint_e2e;
mod typeshed_e2e;
mod uvr_interop;
mod watch_e2e;
