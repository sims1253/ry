use std::sync::Arc;

use ry_checker::{
    Project, ProjectTrace, TraceCompletion, TraceEventKind, TraceFunctionId, TraceOptions,
    TraceReason, TraceTrigger,
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

#[test]
fn late_enable_retains_removed_definition_identity_and_file_filter() {
    fn removed(file_filter: Option<&str>) -> ProjectTrace {
        let mut project = Project::new();
        project.add_file("gone.R".into(), parsed("gone.R", "f <- function() 1L"));
        project.add_file("keep.R".into(), parsed("keep.R", "g <- function() 2L"));
        project.check_incremental(); // Already analyzed without tracing.
        project
            .enable_trace(TraceOptions {
                file: file_filter.map(str::to_owned),
                ..TraceOptions::default()
            })
            .unwrap();
        project.remove_file("gone.R");
        checked(&mut project)
    }

    let full = removed(None);
    assert!(full.events.iter().any(|event| {
        matches!(
            event.kind,
            TraceEventKind::Invalidation {
                reason: TraceReason::RemovedDefinition
            }
        ) && event.function.as_ref().is_some_and(|id| {
            id.table_name == "f" && id.file.path == "gone.R" && id.file.source_sha256.len() == 64
        })
    }));
    let filtered = removed(Some("keep.R"));
    assert!(!filtered.events.iter().any(|event| {
        matches!(
            event.kind,
            TraceEventKind::Invalidation {
                reason: TraceReason::RemovedDefinition
            }
        )
    }));
    assert!(
        filtered
            .events
            .iter()
            .any(|event| { event.file.as_ref().is_some_and(|id| id.path == "keep.R") })
    );
}

#[test]
fn removed_or_renamed_definition_survives_all_check_mode_transitions() {
    fn project() -> Project {
        let mut project = Project::new();
        project.add_file("gone.R".into(), parsed("gone.R", "f <- function() 1L"));
        project.add_file("keep.R".into(), parsed("keep.R", "g <- function() 2L"));
        project
    }

    for initial_is_cold in [false, true] {
        for next_is_cold in [false, true] {
            for enable_before_first_check in [false, true] {
                for rename in [false, true] {
                    for file_filter in [None, Some("keep.R"), Some("gone.R")] {
                        let mut traced = project();
                        if enable_before_first_check {
                            traced
                                .enable_trace(TraceOptions {
                                    file: file_filter.map(str::to_owned),
                                    ..TraceOptions::default()
                                })
                                .unwrap();
                        }
                        if initial_is_cold {
                            traced.check();
                        } else {
                            traced.check_incremental();
                        }
                        traced.take_trace();
                        if !enable_before_first_check {
                            traced
                                .enable_trace(TraceOptions {
                                    file: file_filter.map(str::to_owned),
                                    ..TraceOptions::default()
                                })
                                .unwrap();
                        }
                        if rename {
                            traced.update_file(
                                "gone.R".into(),
                                Arc::new(parsed("gone.R", "renamed <- function() 3L")),
                            );
                        } else {
                            traced.remove_file("gone.R");
                        }
                        let traced_diagnostics = if next_is_cold {
                            traced.check()
                        } else {
                            traced.check_incremental()
                        };
                        let trace = traced.take_trace().unwrap();

                        let mut plain = project();
                        if initial_is_cold {
                            plain.check();
                        } else {
                            plain.check_incremental();
                        }
                        if rename {
                            plain.update_file(
                                "gone.R".into(),
                                Arc::new(parsed("gone.R", "renamed <- function() 3L")),
                            );
                        } else {
                            plain.remove_file("gone.R");
                        }
                        let plain_diagnostics = if next_is_cold {
                            plain.check()
                        } else {
                            plain.check_incremental()
                        };
                        assert_eq!(
                            format!("{traced_diagnostics:?}"),
                            format!("{plain_diagnostics:?}"),
                            "initial cold={initial_is_cold}, next cold={next_is_cold}, early trace={enable_before_first_check}, rename={rename}, filter={file_filter:?}"
                        );

                        let removals: Vec<_> = trace
                            .events
                            .iter()
                            .filter(|event| {
                                matches!(
                                    event.kind,
                                    TraceEventKind::Invalidation {
                                        reason: TraceReason::RemovedDefinition
                                    }
                                )
                            })
                            .collect();
                        if file_filter == Some("keep.R") {
                            assert!(removals.is_empty());
                            assert!(trace.events.iter().any(|event| {
                                event.file.as_ref().is_some_and(|id| id.path == "keep.R")
                            }));
                        } else {
                            assert_eq!(removals.len(), 1);
                            assert!(removals[0].function.as_ref().is_some_and(|id| {
                                id.table_name == "f"
                                    && id.file.path == "gone.R"
                                    && id.file.source_sha256.len() == 64
                            }));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn rename_after_cold_check_records_old_definition_without_changing_diagnostics() {
    let mut traced = pair();
    let mut plain = pair();
    traced.check();
    plain.check();
    traced.enable_trace(TraceOptions::default()).unwrap();
    for project in [&mut traced, &mut plain] {
        project.update_file(
            "leaf.R".into(),
            Arc::new(parsed("leaf.R", "renamed <- function() 2L")),
        );
    }
    assert_eq!(
        format!("{:?}", traced.check_incremental()),
        format!("{:?}", plain.check_incremental())
    );
    let trace = traced.take_trace().unwrap();
    assert!(trace.events.iter().any(|event| {
        matches!(
            event.kind,
            TraceEventKind::Invalidation {
                reason: TraceReason::RemovedDefinition
            }
        ) && event
            .function
            .as_ref()
            .is_some_and(|id| id.table_name == "leaf" && id.file.path == "leaf.R")
    }));
}

#[test]
fn removed_winning_definition_keeps_old_file_identity_when_shadowing_flips() {
    fn changed(
        initial_cold: bool,
        next_cold: bool,
        early_trace: bool,
        file_filter: Option<&str>,
    ) -> ProjectTrace {
        let mut project = Project::new();
        project.add_file("early.R".into(), parsed("early.R", "f <- function() 1L"));
        project.add_file("late.R".into(), parsed("late.R", "f <- function() 2L"));
        let options = TraceOptions {
            file: file_filter.map(str::to_owned),
            ..TraceOptions::default()
        };
        if early_trace {
            project.enable_trace(options.clone()).unwrap();
        }
        if initial_cold {
            project.check();
        } else {
            project.check_incremental();
        }
        project.take_trace();
        if !early_trace {
            project.enable_trace(options).unwrap();
        }
        project.remove_file("late.R");
        if next_cold {
            project.check();
        } else {
            project.check_incremental();
        }
        project.take_trace().unwrap()
    }

    for initial_cold in [false, true] {
        for next_cold in [false, true] {
            for early_trace in [false, true] {
                let full = changed(initial_cold, next_cold, early_trace, None);
                let replaced = full
                    .events
                    .iter()
                    .find(|event| {
                        matches!(
                            event.kind,
                            TraceEventKind::Invalidation {
                                reason: TraceReason::ReplacedDefinition
                            }
                        )
                    })
                    .expect("old winning definition invalidated");
                assert!(
                    replaced
                        .function
                        .as_ref()
                        .is_some_and(|id| { id.table_name == "f" && id.file.path == "late.R" })
                );
                assert!(
                    matches!(&replaced.trigger, Some(TraceTrigger::Function { identity }) if identity.table_name == "f" && identity.file.path == "early.R")
                );
                assert!(
                    changed(initial_cold, next_cold, early_trace, Some("late.R"))
                        .events
                        .iter()
                        .any(|event| {
                            matches!(
                                event.kind,
                                TraceEventKind::Invalidation {
                                    reason: TraceReason::ReplacedDefinition
                                }
                            )
                        })
                );
                assert!(
                    !changed(initial_cold, next_cold, early_trace, Some("early.R"))
                        .events
                        .iter()
                        .any(|event| {
                            matches!(
                                event.kind,
                                TraceEventKind::Invalidation {
                                    reason: TraceReason::ReplacedDefinition
                                }
                            )
                        })
                );
            }
        }
    }
}

#[test]
fn reorder_winner_flip_survives_all_check_modes_and_exact_filters() {
    fn project() -> Project {
        let mut project = Project::new();
        project.add_file("use.R".into(), parsed("use.R", "x <- f() + 1L"));
        project.add_file("z.R".into(), parsed("z.R", "f <- function() 1L"));
        project.add_file("a.R".into(), parsed("a.R", "f <- function() 'str'"));
        project.enable_reference_capture();
        project
    }

    fn check_mode(project: &mut Project, cold: bool) -> String {
        let diagnostics = if cold {
            project.check()
        } else {
            project.check_incremental()
        };
        format!("{diagnostics:?}")
    }

    fn scenario(
        initial_cold: bool,
        next_cold: bool,
        early_trace: bool,
        file: Option<&str>,
        function: Option<TraceFunctionId>,
    ) -> ProjectTrace {
        let mut traced = project();
        let mut plain = project();
        let options = TraceOptions {
            file: file.map(str::to_owned),
            function,
            ..TraceOptions::default()
        };
        if early_trace {
            traced.enable_trace(options.clone()).unwrap();
        }
        assert_eq!(
            check_mode(&mut traced, initial_cold),
            check_mode(&mut plain, initial_cold)
        );
        traced.take_trace();
        traced.take_reference_facts();
        plain.take_reference_facts();
        if !early_trace {
            traced.enable_trace(options).unwrap();
        }
        let order = ["a.R".into(), "use.R".into(), "z.R".into()];
        traced.reorder_files(&order);
        plain.reorder_files(&order);
        assert_eq!(
            check_mode(&mut traced, next_cold),
            check_mode(&mut plain, next_cold),
            "initial cold={initial_cold}, next cold={next_cold}, early trace={early_trace}"
        );
        assert_eq!(
            format!("{:?}", traced.take_reference_facts()),
            format!("{:?}", plain.take_reference_facts())
        );
        traced.take_trace().unwrap()
    }

    for initial_cold in [false, true] {
        for next_cold in [false, true] {
            for early_trace in [false, true] {
                let full = scenario(initial_cold, next_cold, early_trace, None, None);
                let flips: Vec<_> = full
                    .events
                    .iter()
                    .filter(|event| {
                        matches!(
                            event.kind,
                            TraceEventKind::Invalidation {
                                reason: TraceReason::ReplacedDefinition
                            }
                        )
                    })
                    .collect();
                assert_eq!(
                    flips.len(),
                    1,
                    "{initial_cold}/{next_cold}/{early_trace}: {full:?}"
                );
                let old = flips[0].function.as_ref().unwrap().clone();
                assert_eq!(old.table_name, "f");
                assert_eq!(old.file.path, "a.R");
                assert_eq!(old.file.source_sha256.len(), 64);
                let new = match flips[0].trigger.as_ref() {
                    Some(TraceTrigger::Function { identity }) => identity.clone(),
                    other => panic!("new winning definition must trigger event: {other:?}"),
                };
                assert_eq!(new.table_name, "f");
                assert_eq!(new.file.path, "z.R");
                assert_ne!(old.file.source_sha256, new.file.source_sha256);

                for file in [Some("a.R"), Some("z.R"), Some("use.R")] {
                    let filtered = scenario(initial_cold, next_cold, early_trace, file, None);
                    let retained = filtered
                        .events
                        .iter()
                        .filter(|event| {
                            matches!(
                                event.kind,
                                TraceEventKind::Invalidation {
                                    reason: TraceReason::ReplacedDefinition
                                }
                            )
                        })
                        .count();
                    assert_eq!(retained, usize::from(file == Some("a.R")));
                }
                for (function, expected) in [(old, 1), (new, 0)] {
                    let filtered =
                        scenario(initial_cold, next_cold, early_trace, None, Some(function));
                    let retained = filtered
                        .events
                        .iter()
                        .filter(|event| {
                            matches!(
                                event.kind,
                                TraceEventKind::Invalidation {
                                    reason: TraceReason::ReplacedDefinition
                                }
                            )
                        })
                        .count();
                    assert_eq!(retained, expected);
                }
            }
        }
    }
}

#[test]
fn reorder_without_a_winner_flip_has_no_definition_transition() {
    for cold in [false, true] {
        let mut project = Project::new();
        project.add_file("a.R".into(), parsed("a.R", "f <- function() 1L"));
        project.add_file("b.R".into(), parsed("b.R", "g <- function() 2L"));
        project.enable_trace(TraceOptions::default()).unwrap();
        if cold {
            project.check();
        } else {
            project.check_incremental();
        }
        project.take_trace();
        project.reorder_files(&["b.R".into(), "a.R".into()]);
        if cold {
            project.check();
        } else {
            project.check_incremental();
        }
        let trace = project.take_trace().unwrap();
        assert!(!trace.events.iter().any(|event| matches!(
            event.kind,
            TraceEventKind::Invalidation {
                reason: TraceReason::ReplacedDefinition | TraceReason::RemovedDefinition
            }
        )));
    }
}

#[test]
fn scheduling_and_emission_retain_the_actual_cross_file_dependency() {
    fn changed(file_filter: Option<&str>) -> ProjectTrace {
        let mut project = Project::new();
        project.add_file(
            "leaves.R".into(),
            parsed("leaves.R", "b <- function() 1L\nc <- function() 2L"),
        );
        project.add_file(
            "caller.R".into(),
            parsed("caller.R", "a <- function() b() + c()"),
        );
        project
            .enable_trace(TraceOptions {
                file: file_filter.map(str::to_owned),
                ..TraceOptions::default()
            })
            .unwrap();
        checked(&mut project);
        project.update_file(
            "leaves.R".into(),
            Arc::new(parsed(
                "leaves.R",
                "b <- function() 'new'\nc <- function() 'value'",
            )),
        );
        checked(&mut project)
    }

    let full = changed(None);
    let filtered = changed(Some("caller.R"));
    for trace in [&full, &filtered] {
        assert!(trace.events.iter().any(|event| {
            matches!(event.kind, TraceEventKind::Scheduled { round: 0, reason: TraceReason::DependencyRead })
                && event.function.as_ref().is_some_and(|id| id.table_name == "a")
                && matches!(&event.trigger, Some(TraceTrigger::Function { identity }) if identity.table_name == "b" && identity.file.path == "leaves.R")
        }));
        assert!(trace.events.iter().any(|event| {
            matches!(event.kind, TraceEventKind::Scheduled { round: 1, reason: TraceReason::ReturnChanged })
                && event.function.as_ref().is_some_and(|id| id.table_name == "a")
                && matches!(&event.trigger, Some(TraceTrigger::ReturnSlot { bindings, .. }) if bindings.iter().any(|id| id.table_name == "b" && id.file.path == "leaves.R"))
        }));
        assert!(trace.events.iter().any(|event| {
            matches!(event.kind, TraceEventKind::Emission { reason: TraceReason::DependencyRead })
                && event.file.as_ref().is_some_and(|id| id.path == "caller.R")
                && matches!(&event.trigger, Some(TraceTrigger::Function { identity }) if identity.table_name == "b" && identity.file.path == "leaves.R")
        }));
    }
    assert!(
        !filtered
            .events
            .iter()
            .any(|event| { event.file.as_ref().is_some_and(|id| id.path == "leaves.R") })
    );
}
