use std::sync::Arc;

use ry_checker::{
    Project, ProjectTrace, TraceCompletion, TraceEventKind, TraceOptions, TraceReason,
};
use ry_core::{RParser, SourceFile};

fn parsed(path: &str, source: &str) -> SourceFile {
    RParser::new()
        .expect("R parser")
        .parse(path, source)
        .expect("parse test source")
}

fn pair() -> Project {
    let mut project = Project::new();
    project.add_file("leaf.R".into(), parsed("leaf.R", "leaf <- function() 1L"));
    project.add_file(
        "caller.R".into(),
        parsed("caller.R", "caller <- function() leaf()\nx <- caller()"),
    );
    project
}

fn checked(project: &mut Project) -> ProjectTrace {
    project.check_incremental();
    project.take_trace().expect("trace enabled")
}

fn has_file_event(trace: &ProjectTrace, path: &str, reason: TraceReason) -> bool {
    trace.events.iter().any(|event| {
        event.file.as_ref().is_some_and(|file| file.path == path)
            && matches!(event.kind, TraceEventKind::Emission { reason: why } if why == reason)
    })
}

#[test]
fn unchanged_incremental_and_body_edit_report_actual_cache_and_emission() {
    let mut project = pair();
    project.enable_trace(TraceOptions::default()).unwrap();
    let cold = checked(&mut project);
    assert_eq!(cold.summary.collected_files, 2);
    assert_eq!(cold.summary.emitted_files, 2);
    assert_eq!(cold.summary.completion, Some(TraceCompletion::Converged));

    let unchanged = checked(&mut project);
    assert_eq!(unchanged.summary.collection_cache_hits, 2);
    assert_eq!(unchanged.summary.collected_files, 0);
    assert_eq!(unchanged.summary.refined_functions, 0);
    assert_eq!(unchanged.summary.emitted_files, 0);
    assert_eq!(unchanged.summary.emission_cache_hits, 2);
    assert_eq!(
        unchanged.summary.completion,
        Some(TraceCompletion::Converged)
    );
    assert_eq!(cold.input_snapshot_sha256, unchanged.input_snapshot_sha256);

    project.update_file(
        "leaf.R".into(),
        Arc::new(parsed("leaf.R", "leaf <- function() 2L")),
    );
    let edited = checked(&mut project);
    assert_eq!(edited.summary.collected_files, 1);
    assert_eq!(edited.summary.collection_cache_hits, 1);
    assert_eq!(edited.summary.emitted_files, 2);
    assert!(has_file_event(&edited, "leaf.R", TraceReason::DirtySource));
    assert!(has_file_event(
        &edited,
        "caller.R",
        TraceReason::DependencyRead
    ));
}

#[test]
fn signature_change_and_removed_definition_report_causal_reasons() {
    let mut project = Project::new();
    project.add_file("leaf.R".into(), parsed("leaf.R", "leaf <- function(x) x"));
    project.add_file(
        "caller.R".into(),
        parsed("caller.R", "caller <- function() leaf(x = 1L)"),
    );
    project.enable_trace(TraceOptions::default()).unwrap();
    checked(&mut project);

    project.update_file(
        "leaf.R".into(),
        Arc::new(parsed("leaf.R", "leaf <- function(y) y")),
    );
    let signature = checked(&mut project);
    assert!(signature.events.iter().any(|event| {
        event
            .function
            .as_ref()
            .is_some_and(|id| id.table_name == "leaf")
            && matches!(
                event.kind,
                TraceEventKind::FunctionChanged {
                    signature_changed: true,
                    ..
                }
            )
    }));
    assert!(has_file_event(
        &signature,
        "caller.R",
        TraceReason::DependencyRead
    ));

    project.update_file(
        "leaf.R".into(),
        Arc::new(parsed("leaf.R", "other <- function() 1L")),
    );
    let removed = checked(&mut project);
    assert!(removed.events.iter().any(|event| {
        event
            .function
            .as_ref()
            .is_some_and(|id| id.table_name == "leaf")
            && matches!(
                event.kind,
                TraceEventKind::Invalidation {
                    reason: TraceReason::RemovedDefinition
                }
            )
    }));
    assert_eq!(removed.summary.emitted_files, 2);
}

#[test]
fn package_attachment_changes_refinement_and_emission_scope() {
    let mut project = pair();
    project.enable_trace(TraceOptions::default()).unwrap();
    checked(&mut project);
    project.update_file(
        "leaf.R".into(),
        Arc::new(parsed("leaf.R", "library(stats)\nleaf <- function() 1L")),
    );
    let attached = checked(&mut project);
    assert!(attached.events.iter().any(|event| matches!(
        event.kind,
        TraceEventKind::RefinementScope {
            full: true,
            reason: TraceReason::PackageSetChanged,
            ..
        }
    )));
    assert_eq!(attached.summary.emitted_files, 2);
    assert!(has_file_event(
        &attached,
        "caller.R",
        TraceReason::PackageSetChanged
    ));
}

#[test]
fn convergence_and_depth_bound_are_distinct() {
    let mut simple = pair();
    simple.enable_trace(TraceOptions::default()).unwrap();
    assert_eq!(
        checked(&mut simple).summary.completion,
        Some(TraceCompletion::Converged)
    );

    // Sorted names make the return evidence move one caller back per round.
    // Twelve links exceed MAX_FIXPOINT_DEPTH=8 without changing that bound.
    let mut source = String::new();
    for index in 0..12 {
        if index == 11 {
            source.push_str("f11 <- function() 1L\n");
        } else {
            source.push_str(&format!("f{index:02} <- function() f{:02}()\n", index + 1));
        }
    }
    let mut bounded = Project::new();
    bounded.add_file("chain.R".into(), parsed("chain.R", &source));
    bounded.enable_trace(TraceOptions::default()).unwrap();
    let bounded_diagnostics = bounded.check_incremental();
    let trace = bounded.take_trace().unwrap();
    let mut plain = Project::new();
    plain.add_file("chain.R".into(), parsed("chain.R", &source));
    assert_eq!(bounded_diagnostics, plain.check_incremental());
    assert_eq!(
        trace.summary.completion,
        Some(TraceCompletion::BoundReached)
    );
    assert_eq!(trace.summary.refinement_rounds, 8);
    assert!(trace.events.iter().any(|event| matches!(
        event.kind,
        TraceEventKind::FixpointComplete {
            completion: TraceCompletion::BoundReached,
            rounds: 8
        }
    )));
}

#[test]
fn logical_events_are_deterministic_and_budgeted() {
    let mut left = pair();
    let mut right = pair();
    left.enable_trace(TraceOptions::default()).unwrap();
    right.enable_trace(TraceOptions::default()).unwrap();
    let first = checked(&mut left);
    let second = checked(&mut right);
    assert_eq!(first, second);
    assert_eq!(first.analysis_run_id, 1);
    assert_eq!(first.input_snapshot_sha256.len(), 64);
    assert!(
        first
            .events
            .iter()
            .filter_map(|event| event.function.as_ref())
            .all(|id| { id.file.path == "leaf.R" || id.file.path == "caller.R" })
    );

    let mut bounded = pair();
    bounded
        .enable_trace(TraceOptions {
            max_events: 2,
            max_event_bytes: 512,
            ..TraceOptions::default()
        })
        .unwrap();
    let trace = checked(&mut bounded);
    assert_eq!(trace.events.len(), 2);
    assert!(trace.summary.truncated);
    assert!(trace.summary.dropped_events > 0);
    assert!(matches!(
        trace.events.last().map(|event| &event.kind),
        Some(TraceEventKind::Truncated { .. })
    ));
    let bytes = serde_json::to_vec(&trace.events).unwrap().len();
    assert_eq!(trace.summary.event_bytes, bytes);
    assert!(bytes <= 512);

    let mut byte_bounded = pair();
    byte_bounded
        .enable_trace(TraceOptions {
            max_event_bytes: 256,
            ..TraceOptions::default()
        })
        .unwrap();
    let trace = checked(&mut byte_bounded);
    assert!(trace.summary.truncated);
    assert!(trace.summary.event_bytes <= 256);
    assert!(matches!(
        trace.events.last().map(|event| &event.kind),
        Some(TraceEventKind::Truncated { .. })
    ));

    let mut marker_only = pair();
    marker_only
        .enable_trace(TraceOptions {
            max_events: 1,
            max_event_bytes: 256,
            ..TraceOptions::default()
        })
        .unwrap();
    let trace = checked(&mut marker_only);
    assert_eq!(trace.events.len(), 1);
    assert!(matches!(
        trace.events[0].kind,
        TraceEventKind::Truncated { .. }
    ));
    assert_eq!(
        trace.summary.event_bytes,
        serde_json::to_vec(&trace.events).unwrap().len()
    );
    assert!(trace.summary.event_bytes <= 256);
}

#[test]
fn exact_function_and_file_filters_keep_only_selected_local_events() {
    let mut project = pair();
    project.enable_trace(TraceOptions::default()).unwrap();
    let initial = checked(&mut project);
    let caller_id = initial
        .events
        .iter()
        .filter_map(|event| event.function.as_ref())
        .find(|id| id.table_name == "caller")
        .expect("caller ID")
        .clone();
    project
        .enable_trace(TraceOptions {
            file: Some("caller.R".into()),
            function: Some(caller_id.clone()),
            ..TraceOptions::default()
        })
        .unwrap();
    project.update_file(
        "leaf.R".into(),
        Arc::new(parsed("leaf.R", "leaf <- function() 'changed'")),
    );
    let filtered = checked(&mut project);
    assert!(
        filtered
            .events
            .iter()
            .filter_map(|event| event.function.as_ref())
            .all(|id| id == &caller_id)
    );
    assert!(filtered.events.iter().all(|event| event.file.is_none()));
    assert!(filtered.summary.emitted_files >= 1);
}

fn scopes_snapshot(project: &mut Project) -> String {
    let scopes = project
        .take_scope_records()
        .into_iter()
        .map(|(path, records)| {
            let records = records
                .into_iter()
                .map(|record| {
                    let mut bindings: Vec<_> = record
                        .scope
                        .bindings
                        .into_iter()
                        .map(|(name, ty)| (name, ty.to_string()))
                        .collect();
                    bindings.sort_unstable();
                    (record.kind, record.name, record.span, bindings)
                })
                .collect::<Vec<_>>();
            (path, records)
        })
        .collect::<Vec<_>>();
    format!("{scopes:?}")
}

fn facts_snapshot(project: &mut Project) -> (String, String, String) {
    let diagnostics = format!("{:?}", project.check_incremental());
    let scopes = scopes_snapshot(project);
    let references = format!("{:?}", project.take_reference_facts());
    (diagnostics, scopes, references)
}

#[test]
fn trace_does_not_change_diagnostics_or_available_facts() {
    let mut plain = pair();
    let mut traced = pair();
    for project in [&mut plain, &mut traced] {
        project.enable_scope_capture();
        project.enable_reference_capture();
    }
    traced.enable_trace(TraceOptions::default()).unwrap();
    let plain_cold = format!("{:?}", plain.check());
    let traced_cold = format!("{:?}", traced.check());
    assert_eq!(plain_cold, traced_cold);
    assert_eq!(scopes_snapshot(&mut plain), scopes_snapshot(&mut traced));
    assert_eq!(
        format!("{:?}", plain.take_reference_facts()),
        format!("{:?}", traced.take_reference_facts())
    );
    assert!(traced.take_trace().is_some());
    assert_eq!(facts_snapshot(&mut plain), facts_snapshot(&mut traced));
    assert!(traced.take_trace().is_some());

    for project in [&mut plain, &mut traced] {
        project.update_file(
            "leaf.R".into(),
            Arc::new(parsed("leaf.R", "leaf <- function() 'changed'")),
        );
    }
    assert_eq!(facts_snapshot(&mut plain), facts_snapshot(&mut traced));
    assert!(traced.take_trace().is_some());
}
