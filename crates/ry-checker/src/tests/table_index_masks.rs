use super::*;

/// `x[i]` / `x[i, j]` on a table-shaped receiver are data-mask positions:
/// free symbols resolve against the receiver's columns before scope
/// functions (#369). A column named like a function (`month`, `table`,
/// `count`) is data at runtime, so the function interpretation must not
/// drive comparison diagnostics. The same name outside a mask keeps its
/// function typing and diagnostics.
#[test]
fn table_index_columns_shadow_scope_functions() {
    let masked = check(
        "month <- function(x) format(x)\n\
         flights <- data.frame(month = 1:12, day = 1:12, dep_time = 401:412)\n\
         june_i <- flights[month == 6L]\n\
         june_ij <- flights[month == 6L, .(dep_time)]\n\
         june_rows <- flights[month == 6L, ]\n\
         june_col <- flights[month == 6L, \"dep_time\"]\n",
    );
    assert!(
        masked
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY030" | "RY040")),
        "a column named like a scope function must not borrow its type in [i, j]: {masked:?}"
    );

    let outside = check("month <- function(x) format(x)\nx <- month == 6L\n");
    assert!(
        outside.iter().any(|diagnostic| diagnostic.code == "RY030"),
        "outside a mask the same name keeps the function typing: {outside:?}"
    );
}

/// The corpus shape from the issue: the same-named function lives in a
/// sibling file, the receiver is a column-carrying data frame built in
/// this file. The project-wide function table must not win inside the
/// mask.
#[test]
fn project_functions_do_not_shadow_masked_columns() {
    let mut project = Project::new();
    project.add_file(
        "defs.R".to_string(),
        parse_file("defs.R", "month <- function(x) format(x)\n"),
    );
    project.add_file(
        "use.R".to_string(),
        parse_file(
            "use.R",
            "flights <- data.frame(month = 1:12, dep_time = 401:412)\njune <- flights[month == 6L]\n",
        ),
    );
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY030"),
        "a cross-file function must not type a masked column: {diagnostics:?}"
    );
}

/// Without usable column knowledge (an opaque receiver, the shape an
/// unstubbed `as.data.table()` / `fread()` call types as) the mask stays
/// light: bare symbols never borrow a scope function's type (#369's
/// bail-out), and named control arguments (`by`, `.SDcols`) resolve
/// through an unenumerable mask. The receiver may equally be an atomic
/// vector whose type degraded, so positional names that resolve nowhere
/// keep the ordinary RY010.
#[test]
fn unknown_schema_table_index_prefers_silence() {
    let masked = check(
        "month <- function(x) format(x)\n\
         dat <- data.table::as.data.table(data.frame(month = 1:12, dep = 401:412))\n\
         june <- dat[month == 6L]\n\
         cols <- dat[, .(dep)]\n\
         by_pos <- dat[, mean(month), by = day]\n",
    );
    assert!(
        masked
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY030" | "RY040" | "RY010")),
        "an unknown-columns receiver must silence bare-symbol comparisons and controls: {masked:?}"
    );

    let nowhere = check(
        "dat <- data.table::as.data.table(data.frame(month = 1:12))\n\
         x <- dat[undefined_name]\n",
    );
    assert!(
        nowhere.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "a positional name that resolves nowhere keeps the unbound check: {nowhere:?}"
    );
}

/// A mask with a complete schema keeps the function fallback for names
/// the receiver provably does not carry, so genuine function-value uses
/// stay diagnosed inside the mask.
#[test]
fn masked_names_absent_from_a_complete_schema_keep_function_typing() {
    let diagnostics = check(
        "flights <- data.frame(month = 1:12, dep_time = 401:412)\n\
         inside <- with(flights, grep == \"a\")\n\
         indexed <- flights[grep == \"a\", ]\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY030"),
        "a non-column name inside a complete-schema mask is still the function: {diagnostics:?}"
    );
}

/// data.table's `:=` names its targets on the left side: bare symbols
/// and `c()`/`` `:=`() `` spellings denote columns to create or replace,
/// not references. The assigned value stays an ordinary masked
/// expression with its diagnostics.
#[test]
fn table_assign_targets_are_not_references() {
    let quiet = check(
        "month <- function(x) format(x)\n\
         dat <- data.table::as.data.table(data.frame(month = 1:12, dep = 401:412))\n\
         dat[, dep := NA_integer_]\n\
         dat[, june := month == 6L]\n\
         dat[, c(\"first\", \"second\") := list(1L, 2L)]\n\
         dat[, `:=`(x = 1L, y = 2L)]\n",
    );
    assert!(
        quiet.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "`:=` targets are column names, not unbound references: {quiet:?}"
    );
    assert!(
        quiet.iter().all(|diagnostic| diagnostic.code != "RY030"),
        "the `:=` value expression must resolve columns first: {quiet:?}"
    );

    let checked = check(
        "dat <- data.table::as.data.table(data.frame(month = 1:12))\n\
         dat[, new_col := \"a\" + 1L]\n",
    );
    assert!(
        checked.iter().any(|diagnostic| diagnostic.code == "RY040"),
        "the assigned value keeps its type diagnostics: {checked:?}"
    );
}

/// The j-position pronouns `.SD`, `.N`, `.I`, `.BY`, and `.GRP` are
/// supplied by the mask, while `.SDcols` / `by` values resolve through
/// it. Outside the mask the same names stay unbound.
#[test]
fn table_index_pronouns_are_bound_inside_the_mask() {
    let masked = check(
        "dat <- data.table::as.data.table(data.frame(month = 1:12, dep = 401:412))\n\
         dat[, lapply(.SD, max), .SDcols = c(\"month\", \"dep\")]\n\
         dat[, .N, by = month]\n\
         dat[, .I[1L]]\n\
         dat[, .BY$x]\n\
         dat[, .GRP]\n",
    );
    assert!(
        masked.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "j-position pronouns and by/.SDcols values must resolve through the mask: {masked:?}"
    );

    let outside = check("x <- .SD\n");
    assert!(
        outside.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "outside a mask `.SD` stays unbound: {outside:?}"
    );
}

/// The `subset()` / `with()` / dplyr mask paths already resolve columns
/// first; pin them against the same column-vs-function conflict so all
/// data-mask families keep one resolution order (#369).
#[test]
fn verb_masks_resolve_function_named_columns_first() {
    for source in [
        "dat <- data.frame(table = c(\"4\", \"6\"), penalty = c(1, 2))\n\
         s <- subset(dat, table == \"6\")\n\
         w <- with(dat, table == \"6\")\n",
        "suppressMessages(library(dplyr))\n\
         flights <- data.frame(month = 1:12, dep_time = 401:412)\n\
         a <- filter(flights, month == 6L)\n\
         b <- mutate(flights, late = month + dep_time)\n",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.code, "RY030" | "RY040")),
            "verb masks must resolve columns before scope functions: {diagnostics:?}"
        );
    }
}

/// Atomic vectors evaluate their indices eagerly in the calling frame,
/// so their `[` arguments keep the ordinary resolution ladder and its
/// diagnostics. (A `matrix(...)` call types as opaque in ry, so it
/// shares the unknown-receiver bail-out rather than eager semantics.)
#[test]
fn atomic_receivers_keep_eager_index_diagnostics() {
    let vector = check(
        "month <- function(x) format(x)\n\
         v <- c(1.5, 2.5)\n\
         x <- v[month == 1L]\n",
    );
    assert!(
        vector.iter().any(|diagnostic| diagnostic.code == "RY030"),
        "an atomic receiver's index stays an eager expression: {vector:?}"
    );

    let unbound = check("v <- c(1.5, 2.5)\nx <- v[not_a_column]\n");
    assert!(
        unbound.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "an atomic receiver keeps RY010 for unbound index names: {unbound:?}"
    );
}

/// The mask changes name resolution, not expression checking: literal
/// type errors inside masked index arguments still fire.
#[test]
fn masked_index_arguments_keep_type_diagnostics() {
    let diagnostics = check(
        "flights <- data.frame(month = 1:12, dep_time = 401:412)\n\
         bad <- flights[month == 6L, \"a\" + 1L]\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040"),
        "masked arguments must still be inferred: {diagnostics:?}"
    );
}
