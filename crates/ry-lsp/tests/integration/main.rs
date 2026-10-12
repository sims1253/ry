//! LSP integration tests. They live in one test target because each
//! `tests/*.rs` file links its own copy of the whole workspace; add new
//! suites as modules here. `server_replay.rs` stays a separate target: the
//! performance lane builds and times it alone in release mode.

mod closed_file_suppression;
mod code_action_eligibility;
mod config_anchor;
mod configuration_refresh;
mod cross_file_shadowing;
mod diagnostic_reconciliation;
mod disk_index_admission;
mod disk_index_freshness;
mod fallback_isolation;
mod harness;
mod incremental_index_budget;
mod input_boundaries;
mod native_registration;
mod nested_package_isolation;
mod open_document_eligibility;
mod path_rule_overrides;
mod protocol;
mod protocol_contract;
mod report_inputs;
mod serialized_notice;
mod session_state_machine;
mod stale_diagnostic_actions;
mod suppression_edits;
mod typehint_adoption;
mod unused_ignore;
mod versioned_suppression_edits;
mod watched_closed_publish;
mod watched_reconciliation;
mod watched_resolution_context;
