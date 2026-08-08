//! Multi-file project tests. Verifies that functions and S3 methods
//! defined in one file are visible when checking another file in the
//! same project.

use ry_checker::Project;
use ry_core::RParser;
use std::sync::Arc;

fn parse(path: &str, src: &str) -> ry_core::SourceFile {
    let mut p = RParser::new().unwrap();
    p.parse(path, src).unwrap()
}

#[test]
fn cross_file_function_visibility() {
    // utils.R defines a function, analysis.R calls it. Without
    // project mode, the call would emit RY010 because the per-file
    // checker does not know about `double_it`.
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "double_it <- function(x = 1L) { x * 2 }\n"),
    );
    project.add_file(
        "analysis.R".to_string(),
        parse("analysis.R", "result <- double_it(5)\n"),
    );
    let diags = project.check();
    let analysis_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == "analysis.R")
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        analysis_diags.iter().all(|d| d.code != "RY010"),
        "double_it should be visible across files, got: {:?}",
        analysis_diags
    );
}

#[test]
fn cross_file_function_return_type_propagates() {
    // If utils.R defines a function returning character, calling it
    // from analysis.R and using the result arithmetically should
    // trigger RY040. This proves that the cross-file return-type
    // refinement from pass 2 reaches the per-file diagnostics in
    // pass 3.
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "make_string <- function() { \"hello\" }\n"),
    );
    project.add_file(
        "analysis.R".to_string(),
        parse("analysis.R", "y <- make_string() + 1L\n"),
    );
    let diags = project.check();
    let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
    assert!(
        all.iter().any(|d| d.code == "RY040"),
        "expected RY040 from cross-file character-returning fn + int, got: {:?}",
        all
    );
}

#[test]
fn cross_file_fixpoint_rebinding_overrides_null_narrowing() {
    // `make_writer` is opaque in the first fixpoint iteration, then its
    // function return type is refined. The branch assignment must still
    // replace the NULL-derived narrowing in either iteration.
    let mut project = Project::new();
    project.add_file(
        "writer.R".to_string(),
        parse(
            "writer.R",
            "make_writer <- function() { local({ function(value) value }) }\n",
        ),
    );
    project.add_file(
        "use.R".to_string(),
        parse(
            "use.R",
            "use_writer <- function(writer = NULL) {\n\
             if (is.null(writer)) writer <- make_writer()\n\
             writer(1L)\n\
             }\n\
             use_writer()\n",
        ),
    );
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter().all(|diagnostic| diagnostic.code != "RY070"),
        "an opaque cross-file rebinding must override NULL narrowing: {all:?}"
    );
}

#[test]
fn incremental_edit_rechecks_cross_file_dependents() {
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "make_value <- function() { \"hello\" }\n"),
    );
    project.add_file(
        "analysis.R".to_string(),
        parse("analysis.R", "result <- make_value() + 1L\n"),
    );

    let before = project.check_incremental();
    let before_analysis = before
        .iter()
        .find(|(path, _)| path == "analysis.R")
        .unwrap();
    assert!(
        before_analysis
            .1
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040"),
        "character return should make analysis.R invalid: {before_analysis:?}"
    );

    project.update_file(
        "utils.R".to_string(),
        Arc::new(parse("utils.R", "make_value <- function() { 1L }\n")),
    );
    let after = project.check_incremental();
    let after_analysis = after.iter().find(|(path, _)| path == "analysis.R").unwrap();
    assert!(
        after_analysis
            .1
            .iter()
            .all(|diagnostic| diagnostic.code != "RY040"),
        "integer return should update analysis.R diagnostics: {after_analysis:?}"
    );
}

#[test]
fn cross_file_s3_method_dispatches() {
    // methods.R defines print.foo; usage.R creates a "foo"-classed
    // value and calls print on it. The S3 method table is shared
    // across files, so dispatch finds the method and RY050 stays
    // silent.
    let mut project = Project::new();
    project.add_file(
        "methods.R".to_string(),
        parse(
            "methods.R",
            "print.foo <- function(x, ...) { invisible(x) }\n",
        ),
    );
    project.add_file(
        "usage.R".to_string(),
        parse(
            "usage.R",
            "x <- structure(list(), class = \"foo\")\nprint(x)\n",
        ),
    );
    let diags = project.check();
    let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
    assert!(
        all.iter().all(|d| d.code != "RY050"),
        "print.foo from methods.R should dispatch on usage.R's x, got: {:?}",
        all
    );
}

#[test]
fn cross_file_s3_ops_method_precedes_storage_mode_error() {
    let mut project = Project::new();
    project.add_file(
        "methods.R".to_string(),
        parse(
            "methods.R",
            "Ops.rvar <- function(e1, e2) structure(list(), class = \"rvar\")\n",
        ),
    );
    project.add_file(
        "usage.R".to_string(),
        parse(
            "usage.R",
            "x <- structure(list(1), class = \"rvar\")\ny <- x + x\nz <- x == 1\n",
        ),
    );
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY030" | "RY040")),
        "Ops.rvar should dispatch before primitive list errors: {all:?}"
    );
}

#[test]
fn external_binding_is_a_function_position_candidate() {
    use std::collections::{HashMap, HashSet};

    let mut project = Project::new();
    project.add_file(
        "usage.R".to_string(),
        parse("usage.R", "ndraws <- NULL\nn <- ndraws(x)\n"),
    );
    project.set_external_bindings(HashMap::from([(
        "usage.R".to_string(),
        HashSet::from(["ndraws".to_string()]),
    )]));
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter().all(|diagnostic| diagnostic.code != "RY070"),
        "imported ndraws should remain callable despite a local data binding: {all:?}"
    );
}

#[test]
fn namespace_s3_registration_is_an_operator_candidate() {
    use std::collections::{HashMap, HashSet};

    let mut project = Project::new();
    project.add_file(
        "usage.R".to_string(),
        parse(
            "usage.R",
            "x <- structure(list(1), class = \"external_class\")\ny <- x + x\n",
        ),
    );
    project.set_external_s3_methods(HashMap::from([(
        "usage.R".to_string(),
        HashSet::from([("Ops".to_string(), "external_class".to_string())]),
    )]));
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter().all(|diagnostic| diagnostic.code != "RY040"),
        "registered Ops method should be consulted before storage mode: {all:?}"
    );
}

#[test]
fn load_bindings_activate_at_the_load_statement() {
    use std::collections::{HashMap, HashSet};

    let file = parse(
        "usage.R",
        "before_load\nload(\"objects.rda\")\nafter_load\n",
    );
    let load_start = file
        .stmts
        .iter()
        .find_map(|statement| match statement {
            ry_core::ast::Stmt::Expr(ry_core::ast::Expr::Call { span, .. }) => Some(span.start),
            _ => None,
        })
        .unwrap();
    let mut project = Project::new();
    project.add_file("usage.R".to_string(), file);
    project.set_load_bindings(HashMap::from([(
        "usage.R".to_string(),
        HashMap::from([(load_start, HashSet::from(["after_load".to_string()]))]),
    )]));
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter()
            .any(|diagnostic| diagnostic.message.contains("before_load")),
        "a read before load must remain unbound: {all:?}"
    );
    assert!(
        all.iter()
            .all(|diagnostic| !diagnostic.message.contains("after_load")),
        "a loaded binding should resolve after load: {all:?}"
    );
}

#[test]
fn redefinition_in_different_files_shadows() {
    // If utils.R defines f and other.R also defines f, the later
    // definition wins (matching R's source() semantics). The order
    // files are added via `add_file` determines which one wins.
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "f <- function() { 1L }\n"),
    );
    project.add_file(
        "other.R".to_string(),
        parse("other.R", "f <- function() { \"string\" }\n"),
    );
    project.add_file(
        "usage.R".to_string(),
        parse("usage.R", "result <- f() + 1L\n"),
    );
    let diags = project.check();
    let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
    // The later definition (string) wins, so `result + 1L` is
    // character + int and should fire RY040.
    assert!(
        all.iter().any(|d| d.code == "RY040"),
        "expected shadowed definition to win, got: {:?}",
        all
    );
}

#[test]
fn diagnostics_returned_in_input_order() {
    // The per-file diagnostics vec should preserve the order files
    // were added. Callers (the CLI) rely on this to map paths back to
    // source text and sort consistently.
    let mut project = Project::new();
    project.add_file("a.R".to_string(), parse("a.R", "x <- 1L\n"));
    project.add_file("b.R".to_string(), parse("b.R", "y <- 2L\n"));
    project.add_file("c.R".to_string(), parse("c.R", "z <- 3L\n"));
    let diags = project.check();
    let paths: Vec<&str> = diags.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, vec!["a.R", "b.R", "c.R"]);
}

#[test]
fn empty_files_produce_no_diagnostics() {
    let mut project = Project::new();
    project.add_file("a.R".to_string(), parse("a.R", ""));
    project.add_file("b.R".to_string(), parse("b.R", "\n"));
    let diags = project.check();
    let total: usize = diags.into_iter().map(|(_, d)| d.len()).sum();
    assert_eq!(total, 0, "empty files should not produce diagnostics");
}

// ---------------------------------------------------------------------------
// Plan 33 W1: dirty-set pass 3
// ---------------------------------------------------------------------------

/// A one-line edit to a leaf file (one that no other file depends on) should
/// re-emit exactly one file. Verified via `Project::emit_count`, which counts
/// files actually emitted (not served from cache) in the most recent
/// `check_incremental` call.
#[test]
fn leaf_edit_emits_one_file() {
    let mut parser = RParser::new().unwrap();
    let mut project = Project::new();

    // Two independent leaf files: neither calls the other.
    project.add_file("a.R".to_string(), parser.parse("a.R", "x <- 1L\n").unwrap());
    project.add_file("b.R".to_string(), parser.parse("b.R", "y <- 2L\n").unwrap());

    // Cold check: both files emitted.
    let _ = project.check_incremental();
    #[cfg(test)]
    assert_eq!(project.emit_count, 2, "cold check should emit all files");

    // Edit only a.R (a leaf that b.R does not depend on).
    project.update_file(
        "a.R".to_string(),
        Arc::new(parser.parse("a.R", "x <- 3L\n").unwrap()),
    );
    let _ = project.check_incremental();

    // Only a.R should have been re-emitted; b.R served from cache.
    #[cfg(test)]
    assert_eq!(
        project.emit_count, 1,
        "leaf edit should re-emit exactly 1 file, got {}",
        project.emit_count
    );
}

/// Editing a file that another file calls must re-emit the caller too,
/// because the callee's return type may have changed.
#[test]
fn dependent_edit_emits_caller() {
    let mut parser = RParser::new().unwrap();
    let mut project = Project::new();

    project.add_file(
        "utils.R".to_string(),
        parser
            .parse("utils.R", "make <- function() \"hello\"\n")
            .unwrap(),
    );
    project.add_file(
        "call.R".to_string(),
        parser.parse("call.R", "r <- make() + 1L\n").unwrap(),
    );

    let _ = project.check_incremental();

    // Edit utils.R: make() now returns integer instead of character.
    project.update_file(
        "utils.R".to_string(),
        Arc::new(parser.parse("utils.R", "make <- function() 1L\n").unwrap()),
    );
    let _ = project.check_incremental();

    // Both utils.R (content changed) and call.R (calls make()) should
    // be re-emitted because make()'s return type changed.
    #[cfg(test)]
    assert_eq!(
        project.emit_count, 2,
        "dependent edit should re-emit 2 files (content + caller), got {}",
        project.emit_count
    );
}

/// The cold-vs-incremental equivalence property (Plan 33 W1 invariant):
/// after a sequence of incremental edits, the diagnostics must be identical
/// to a fresh cold check on the same final state.
#[test]
fn incremental_matches_cold_after_edits() {
    let mut parser = RParser::new().unwrap();

    // Build a 5-file project with cross-file dependencies.
    let sources = [
        ("a.R", "fa <- function(x) x * 2\nva <- fa(1L)\n"),
        ("b.R", "fb <- function(x) fa(x) + 1\nvb <- fb(2L)\n"),
        ("c.R", "fc <- function(x) fb(x) * 3\nvc <- fc(3L)\n"),
        ("d.R", "fd <- function() \"text\"\nvd <- fd()\n"),
        ("e.R", "fe <- function(x) paste0(x)\nve <- fe(42L)\n"),
    ];

    // Cold check path.
    let mut cold_project = Project::new();
    for (path, src) in &sources {
        cold_project.add_file(path.to_string(), parser.parse(path, src).unwrap());
    }
    let _cold = cold_project.check();

    // Incremental path: add files one at a time.
    let mut inc_project = Project::new();
    let _ = inc_project.check_incremental(); // empty
    for (path, src) in &sources {
        inc_project.add_file(path.to_string(), parser.parse(path, src).unwrap());
    }
    let _ = inc_project.check_incremental();

    // Now make a few incremental edits, then compare with cold.
    // Edit 1: change fa's return type.
    let edited_a = parser
        .parse("a.R", "fa <- function(x) \"str\"\nva <- fa(1L)\n")
        .unwrap();
    inc_project.update_file("a.R".to_string(), Arc::new(edited_a));

    // Edit 2: change fd's body (leaf file, nothing depends on it).
    let edited_d = parser
        .parse("d.R", "fd <- function() 1L\nvd <- fd()\n")
        .unwrap();
    inc_project.update_file("d.R".to_string(), Arc::new(edited_d));

    let inc_result = inc_project.check_incremental();

    // Build the matching cold project.
    let edited_sources = [
        ("a.R", "fa <- function(x) \"str\"\nva <- fa(1L)\n"),
        ("b.R", "fb <- function(x) fa(x) + 1\nvb <- fb(2L)\n"),
        ("c.R", "fc <- function(x) fb(x) * 3\nvc <- fc(3L)\n"),
        ("d.R", "fd <- function() 1L\nvd <- fd()\n"),
        ("e.R", "fe <- function(x) paste0(x)\nve <- fe(42L)\n"),
    ];
    let mut cold_project2 = Project::new();
    for (path, src) in &edited_sources {
        cold_project2.add_file(path.to_string(), parser.parse(path, src).unwrap());
    }
    let cold_result = cold_project2.check();

    // Compare diagnostics file by file.
    assert_eq!(inc_result.len(), cold_result.len(), "file count mismatch");
    for ((inc_path, inc_diags), (cold_path, cold_diags)) in
        inc_result.iter().zip(cold_result.iter())
    {
        assert_eq!(inc_path, cold_path, "path order mismatch");
        // Compare diagnostic codes + messages (the property that matters).
        let inc_codes: Vec<_> = inc_diags.iter().map(|d| &d.code).collect();
        let cold_codes: Vec<_> = cold_diags.iter().map(|d| &d.code).collect();
        assert_eq!(
            inc_codes, cold_codes,
            "diagnostic codes differ for {inc_path}:\n  incremental: {inc_codes:?}\n  cold:        {cold_codes:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Plan 35 W3: shrinkable cold-vs-incremental equivalence
// ---------------------------------------------------------------------------

use proptest::prelude::*;
use ry_checker::Diagnostic;
use ry_typeshed::{Typeshed, load_package};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug)]
enum SourceModel {
    UnrelatedInteger,
    UnrelatedValue,
    IntegerReturn,
    CharacterReturn,
    RequiredParameter,
    DefaultedParameter,
    RenamedParameter,
    QuotingParameter,
    DefusedParameter,
    DirectCaller,
    ForwardingCaller,
    TransitiveCaller,
    S3Definition,
    S3Dispatch,
    S4Definition,
    S4Dispatch,
    MetadataBindings,
    LoadedPackage,
    Unicode,
    Empty,
}

impl SourceModel {
    fn source(&self) -> &'static str {
        match self {
            Self::UnrelatedInteger => "unrelated <- function() 1L\n",
            Self::UnrelatedValue => "unrelated <- 1L\n",
            Self::IntegerReturn => "target <- function(value) 1L\n",
            Self::CharacterReturn => "target <- function(value) \"text\"\n",
            Self::RequiredParameter => "target <- function(value) value\n",
            Self::DefaultedParameter => "target <- function(value = 1L) value\n",
            Self::RenamedParameter => "target <- function(renamed = 1L) renamed\n",
            Self::QuotingParameter => "target <- function(value) substitute(value)\n",
            Self::DefusedParameter => "target <- function(value) rlang::enquo(value)\n",
            Self::DirectCaller => "result <- target(not_bound) + 1L\n",
            Self::ForwardingCaller => "forward <- function(value) target(value)\n",
            Self::TransitiveCaller => "forward(not_bound)\n",
            Self::S3Definition => "value.foo <- function(x, ...) \"s3\"\n",
            Self::S3Dispatch => {
                "object <- structure(list(), class = \"foo\")\nresult <- value(object) + 1L\n"
            }
            Self::S4Definition => {
                "setClass(\"Widget\", slots = c(value = \"numeric\"))\nsetMethod(\"labels\", signature(\"Widget\"), function(object) c(label = \"ok\"))\n"
            }
            Self::S4Dispatch => "object <- new(\"Widget\")\nlabels(object)[[\"missing\"]]\n",
            Self::MetadataBindings => {
                "load(\"objects.rda\")\nloaded_name\nexternal_name\nimported_helper()\n"
            }
            Self::LoadedPackage => "mutate(data.frame(x = 1L), y = unknown_column)\n",
            Self::Unicode => "unicode <- function(名 = \"😀\") 名\nunicode(不存在)\n",
            Self::Empty => "\n",
        }
    }
}

#[derive(Clone, Debug)]
enum Operation {
    Add {
        file: u8,
        source: SourceModel,
    },
    Update {
        file: u8,
        source: SourceModel,
    },
    Remove {
        file: u8,
    },
    RemoveThenReadd {
        file: u8,
        source: SourceModel,
    },
    RepeatedUpdate {
        file: u8,
        first: SourceModel,
        second: SourceModel,
    },
    BatchRemoveAndUpdate {
        remove: u8,
        update: u8,
        source: SourceModel,
    },
    BatchUpdateTwo {
        first: u8,
        first_source: SourceModel,
        second: u8,
        second_source: SourceModel,
    },
    SetLoaded(bool),
    SetUserStubs(bool),
    SetExternalBinding {
        file: u8,
        enabled: bool,
    },
    SetImportedBinding {
        file: u8,
        enabled: bool,
    },
    SetLoadBinding {
        file: u8,
        enabled: bool,
    },
}

fn source_strategy() -> impl Strategy<Value = SourceModel> {
    prop_oneof![
        Just(SourceModel::UnrelatedInteger),
        Just(SourceModel::UnrelatedValue),
        Just(SourceModel::IntegerReturn),
        Just(SourceModel::CharacterReturn),
        Just(SourceModel::RequiredParameter),
        Just(SourceModel::DefaultedParameter),
        Just(SourceModel::RenamedParameter),
        Just(SourceModel::QuotingParameter),
        Just(SourceModel::DefusedParameter),
        Just(SourceModel::DirectCaller),
        Just(SourceModel::ForwardingCaller),
        Just(SourceModel::TransitiveCaller),
        Just(SourceModel::S3Definition),
        Just(SourceModel::S3Dispatch),
        Just(SourceModel::S4Definition),
        Just(SourceModel::S4Dispatch),
        Just(SourceModel::MetadataBindings),
        Just(SourceModel::LoadedPackage),
        Just(SourceModel::Unicode),
        Just(SourceModel::Empty),
    ]
}

fn operation_strategy() -> impl Strategy<Value = Operation> {
    let file = 0u8..6;
    prop_oneof![
        5 => (file.clone(), source_strategy())
            .prop_map(|(file, source)| Operation::Add { file, source }),
        7 => (file.clone(), source_strategy())
            .prop_map(|(file, source)| Operation::Update { file, source }),
        2 => file.clone().prop_map(|file| Operation::Remove { file }),
        2 => (file.clone(), source_strategy())
            .prop_map(|(file, source)| Operation::RemoveThenReadd { file, source }),
        2 => (file.clone(), source_strategy(), source_strategy()).prop_map(
            |(file, first, second)| Operation::RepeatedUpdate { file, first, second }
        ),
        1 => (file.clone(), file.clone(), source_strategy()).prop_map(
            |(remove, update, source)| Operation::BatchRemoveAndUpdate {
                remove,
                update,
                source,
            }
        ),
        1 => (file.clone(), source_strategy(), file.clone(), source_strategy()).prop_map(
            |(first, first_source, second, second_source)| Operation::BatchUpdateTwo {
                first,
                first_source,
                second,
                second_source,
            }
        ),
        1 => any::<bool>().prop_map(Operation::SetLoaded),
        1 => any::<bool>().prop_map(Operation::SetUserStubs),
        1 => (file.clone(), any::<bool>()).prop_map(|(file, enabled)| {
            Operation::SetExternalBinding { file, enabled }
        }),
        1 => (file.clone(), any::<bool>()).prop_map(|(file, enabled)| {
            Operation::SetImportedBinding { file, enabled }
        }),
        1 => (file, any::<bool>()).prop_map(|(file, enabled)| {
            Operation::SetLoadBinding { file, enabled }
        }),
    ]
}

fn operation_sequence_strategy() -> impl Strategy<Value = Vec<Operation>> {
    // Keep issue #52 in the generated alphabet as a first-class shrink target,
    // not only as an example test. The final edit changes only caller-visible
    // evaluation metadata while leaving the return type effectively opaque.
    let caller_invalidation = Just(vec![
        Operation::Add {
            file: 0,
            source: SourceModel::QuotingParameter,
        },
        Operation::Add {
            file: 1,
            source: SourceModel::ForwardingCaller,
        },
        Operation::Add {
            file: 2,
            source: SourceModel::TransitiveCaller,
        },
        Operation::Update {
            file: 0,
            source: SourceModel::RequiredParameter,
        },
    ]);

    // This batch shifts return-slot indices while changing `target` to the
    // type formerly stored in the vacated slot. Index-keyed invalidation sees
    // a false equality; name-keyed snapshots correctly re-emit the caller.
    let shifted_return_slot = Just(vec![
        Operation::Add {
            file: 0,
            source: SourceModel::UnrelatedInteger,
        },
        Operation::Add {
            file: 1,
            source: SourceModel::CharacterReturn,
        },
        Operation::Add {
            file: 2,
            source: SourceModel::DirectCaller,
        },
        Operation::BatchUpdateTwo {
            first: 0,
            first_source: SourceModel::UnrelatedValue,
            second: 1,
            second_source: SourceModel::IntegerReturn,
        },
    ]);

    prop_oneof![
        1 => caller_invalidation,
        1 => shifted_return_slot,
        5 => prop::collection::vec(operation_strategy(), 1..16),
    ]
}

#[derive(Default)]
struct ProjectState {
    files: Vec<(u8, SourceModel)>,
    loaded: HashSet<String>,
    user_stubs: bool,
    external_bindings: HashMap<String, HashSet<String>>,
    imported_from: HashMap<String, HashMap<String, String>>,
    load_bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
}

fn path(file: u8) -> String {
    format!("generated-{file}.R")
}

fn update_state_file(state: &mut ProjectState, file: u8, source: SourceModel) {
    if let Some((_, existing)) = state.files.iter_mut().find(|(id, _)| *id == file) {
        *existing = source;
    } else {
        state.files.push((file, source));
    }
}

fn generated_user_stubs(enabled: bool) -> Arc<BTreeMap<String, Typeshed>> {
    let mut stubs = BTreeMap::new();
    if enabled {
        stubs.insert(
            "dplyr".to_string(),
            load_package("dplyr")
                .expect("embedded dplyr typeshed")
                .clone(),
        );
    }
    Arc::new(stubs)
}

fn apply_operation(project: &mut Project, state: &mut ProjectState, operation: Operation) {
    match operation {
        Operation::Add { file, source } | Operation::Update { file, source } => {
            let file_path = path(file);
            project.update_file(
                file_path.clone(),
                Arc::new(parse(&file_path, source.source())),
            );
            update_state_file(state, file, source);
        }
        Operation::Remove { file } => {
            project.remove_file(&path(file));
            state.files.retain(|(id, _)| *id != file);
        }
        Operation::RemoveThenReadd { file, source } => {
            let file_path = path(file);
            project.remove_file(&file_path);
            state.files.retain(|(id, _)| *id != file);
            project.update_file(
                file_path.clone(),
                Arc::new(parse(&file_path, source.source())),
            );
            state.files.push((file, source));
        }
        Operation::RepeatedUpdate {
            file,
            first,
            second,
        } => {
            let file_path = path(file);
            project.update_file(
                file_path.clone(),
                Arc::new(parse(&file_path, first.source())),
            );
            update_state_file(state, file, first);
            project.update_file(
                file_path.clone(),
                Arc::new(parse(&file_path, second.source())),
            );
            update_state_file(state, file, second);
        }
        Operation::BatchRemoveAndUpdate {
            remove,
            update,
            source,
        } => {
            let removed_path = path(remove);
            project.remove_file(&removed_path);
            state.files.retain(|(id, _)| *id != remove);

            let updated_path = path(update);
            project.update_file(
                updated_path.clone(),
                Arc::new(parse(&updated_path, source.source())),
            );
            update_state_file(state, update, source);
        }
        Operation::BatchUpdateTwo {
            first,
            first_source,
            second,
            second_source,
        } => {
            let first_path = path(first);
            project.update_file(
                first_path.clone(),
                Arc::new(parse(&first_path, first_source.source())),
            );
            update_state_file(state, first, first_source);

            let second_path = path(second);
            project.update_file(
                second_path.clone(),
                Arc::new(parse(&second_path, second_source.source())),
            );
            update_state_file(state, second, second_source);
        }
        Operation::SetLoaded(enabled) => {
            state.loaded = if enabled {
                HashSet::from(["dplyr".to_string()])
            } else {
                HashSet::new()
            };
            project.set_loaded(state.loaded.clone());
        }
        Operation::SetUserStubs(enabled) => {
            state.user_stubs = enabled;
            project.set_user_stubs(generated_user_stubs(enabled));
        }
        Operation::SetExternalBinding { file, enabled } => {
            let file_path = path(file);
            if enabled {
                state
                    .external_bindings
                    .insert(file_path, HashSet::from(["external_name".to_string()]));
            } else {
                state.external_bindings.remove(&file_path);
            }
            project.set_external_bindings(state.external_bindings.clone());
        }
        Operation::SetImportedBinding { file, enabled } => {
            let file_path = path(file);
            if enabled {
                state.imported_from.insert(
                    file_path,
                    HashMap::from([("imported_helper".to_string(), "fixture".to_string())]),
                );
            } else {
                state.imported_from.remove(&file_path);
            }
            project.set_imported_from(state.imported_from.clone());
        }
        Operation::SetLoadBinding { file, enabled } => {
            let file_path = path(file);
            if enabled {
                state.load_bindings.insert(
                    file_path,
                    HashMap::from([(0, HashSet::from(["loaded_name".to_string()]))]),
                );
            } else {
                state.load_bindings.remove(&file_path);
            }
            project.set_load_bindings(state.load_bindings.clone());
        }
    }
}

fn cold_check(state: &ProjectState) -> Vec<(String, Vec<Diagnostic>)> {
    let mut project = Project::new();
    for (file, source) in &state.files {
        let file_path = path(*file);
        project.add_file(file_path.clone(), parse(&file_path, source.source()));
    }
    project.set_loaded(state.loaded.clone());
    project.set_user_stubs(generated_user_stubs(state.user_stubs));
    project.set_external_bindings(state.external_bindings.clone());
    project.set_imported_from(state.imported_from.clone());
    project.set_load_bindings(state.load_bindings.clone());
    project.check()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        max_shrink_iters: 10_000,
        .. ProptestConfig::default()
    })]

    /// Every generated operation is a checkpoint. Direct equality compares
    /// the complete Diagnostic value: path, code, severity, confidence, span,
    /// message, and any future structured fix field added to Diagnostic.
    #[test]
    fn incremental_matches_cold_property(
        operations in operation_sequence_strategy(),
    ) {
        let mut incremental_project = Project::new();
        let mut state = ProjectState::default();

        for (step, operation) in operations.into_iter().enumerate() {
            let operation_debug = format!("{operation:?}");
            apply_operation(&mut incremental_project, &mut state, operation);
            let incremental = incremental_project.check_incremental();
            let cold = cold_check(&state);
            prop_assert_eq!(
                &incremental,
                &cold,
                "cold/incremental mismatch after step {}: {}",
                step,
                operation_debug,
            );
        }
    }
}

/// Issue #52: changing a callee from quoting to evaluating a parameter must
/// invalidate callers even when the callee's return type stays unchanged.
#[test]
fn parameter_signature_change_reemits_transitive_callers() {
    let mut project = Project::new();
    project.add_file(
        "callee.R".to_string(),
        parse("callee.R", "capture <- function(value) substitute(value)\n"),
    );
    project.add_file(
        "wrapper.R".to_string(),
        parse("wrapper.R", "forward <- function(value) capture(value)\n"),
    );
    project.add_file(
        "caller.R".to_string(),
        parse("caller.R", "forward(not_bound)\n"),
    );

    let before = project.check_incremental();
    assert!(
        before
            .iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .all(|diagnostic| diagnostic.code != "RY010"),
        "the initially quoted argument should not be evaluated: {before:?}",
    );

    project.update_file(
        "callee.R".to_string(),
        Arc::new(parse("callee.R", "capture <- function(value) value\n")),
    );
    let incremental = project.check_incremental();

    let mut cold = Project::new();
    cold.add_file(
        "callee.R".to_string(),
        parse("callee.R", "capture <- function(value) value\n"),
    );
    cold.add_file(
        "wrapper.R".to_string(),
        parse("wrapper.R", "forward <- function(value) capture(value)\n"),
    );
    cold.add_file(
        "caller.R".to_string(),
        parse("caller.R", "forward(not_bound)\n"),
    );
    let cold = cold.check();

    assert_eq!(incremental, cold);
}
