use super::*;

fn network_warnings(source: &str) -> usize {
    check(source).iter().filter(|d| d.code == "RY124").count()
}

fn network_warnings_with(source: &str, setup: impl FnOnce(&mut Checker)) -> usize {
    check_with(source, setup)
        .iter()
        .filter(|d| d.code == "RY124")
        .count()
}

#[test]
fn top_level_and_load_hooks_warn() {
    for source in [
        // The fitzRoy shape: a package data object computed by a
        // download while the namespace is built.
        "season <- utils::download.file('https://example.invalid/x.csv', tempfile())",
        "utils::download.file('https://example.invalid/x.csv', tempfile())",
        "local(utils::download.file('https://example.invalid/x.csv', tempfile()))",
        ".onLoad <- function(libname, pkgname) utils::download.file('https://example.invalid/x', tempfile())",
        ".onAttach <- function(libname, pkgname) { curl::curl_download('https://example.invalid/x', tempfile()) }",
        "httr::GET('https://example.invalid/x')",
        "httr::POST('https://example.invalid/x')",
        "curl::curl_fetch_memory('https://example.invalid/x')",
        "httr2::req_perform(httr2::request('https://example.invalid/x'))",
        "base::socketConnection('example.invalid', port = 80L)",
        "base::url('https://example.invalid/x', open = 'r')",
        "curl::curl('https://example.invalid/x', open = 'r')",
    ] {
        assert_eq!(network_warnings(source), 1, "{source}");
    }
}

#[test]
fn bare_imported_heads_warn_but_bare_unknown_packages_do_not() {
    assert_eq!(
        network_warnings_with(
            "download.file('https://example.invalid/x', tempfile())",
            |c| c.set_imported_from(HashMap::from([(
                "download.file".to_string(),
                "utils".to_string()
            )])),
        ),
        1
    );
    assert_eq!(
        network_warnings_with(
            "curl_download('https://example.invalid/x', tempfile())",
            |c| c.set_imported_from(HashMap::from([(
                "curl_download".to_string(),
                "curl".to_string()
            )])),
        ),
        1
    );
    assert_eq!(
        network_warnings_with(
            "curl_download('https://example.invalid/x', tempfile())",
            |c| c.set_loaded(HashSet::from(["curl".to_string()])),
        ),
        1
    );
    // No import or attachment proves this spelling is curl's function.
    assert_eq!(
        network_warnings("curl_download('https://example.invalid/x', tempfile())"),
        0
    );
}

#[test]
fn ordinary_functions_defaults_and_deferred_connections_stay_quiet() {
    for source in [
        "fetch <- function(u) utils::download.file(u, tempfile())",
        "fetch <- function(u = utils::download.file('https://example.invalid/x', tempfile())) u",
        // `url()` and `curl()` do not connect until opened.
        "con <- base::url('https://example.invalid/x')",
        "con <- base::url('https://example.invalid/x', open = '')",
        "con <- curl::curl('https://example.invalid/x')",
        "con <- base::socketConnection('example.invalid', port = 80L, open = '')",
        "con <- base::url('https://example.invalid/x', open = mode)",
        // A nested closure in a load hook runs only when called.
        ".onLoad <- function(libname, pkgname) { later <- function() utils::download.file('https://example.invalid/x', tempfile()) }",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
}

#[test]
fn guarded_calls_stay_quiet() {
    for source in [
        // Dead bodies never run.
        "if (FALSE) utils::download.file('https://example.invalid/x', tempfile())",
        "while (FALSE) utils::download.file('https://example.invalid/x', tempfile())",
        // Vectorized `&` does not short-circuit: its rhs always runs
        // and must warn (asserted separately below).
        "if (interactive()) utils::download.file('https://example.invalid/x', tempfile())",
        "if (isTRUE(getOption('mypkg.download'))) utils::download.file('https://example.invalid/x', tempfile())",
        "if (nzchar(Sys.getenv('MYPKG_DOWNLOAD'))) utils::download.file('https://example.invalid/x', tempfile())",
        ".onLoad <- function(libname, pkgname) { if (!interactive()) return(invisible()); utils::download.file('https://example.invalid/x', tempfile()) }",
        // Every disjunct is an opt-in probe: whichever one holds, the
        // then-arm is guarded.
        "if (getOption('mypkg.download') || interactive()) utils::download.file('https://example.invalid/x', tempfile())",
        "if (Sys.getenv('MYPKG_DL') != '' || interactive()) utils::download.file('https://example.invalid/x', tempfile())",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
    // A non-probe disjunct may be the true one: not guarded.
    assert_eq!(
        network_warnings(
            "if (file.exists('x') || interactive()) utils::download.file('https://example.invalid/x', tempfile())"
        ),
        1
    );
    // The arm that runs when the probe failed is the unguarded one.
    assert_eq!(
        network_warnings(
            "if (interactive()) NULL else utils::download.file('https://example.invalid/x', tempfile())"
        ),
        1
    );
}

#[test]
fn masked_and_dynamic_callees_stay_quiet() {
    for source in [
        "download.file <- function(url, dest) dest; download.file('https://example.invalid/x', tempfile())",
        "GET <- function(url) url; GET('https://example.invalid/x')",
        "get('download.file', envir = asNamespace('utils'))('https://example.invalid/x', tempfile())",
        "f <- utils::download.file; f('https://example.invalid/x', tempfile())",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
    // A same-named local function inside a wrapper masks the head too.
    assert_eq!(
        network_warnings(
            "download.file <- function(url, dest) dest\nfetch <- function() download.file('https://example.invalid/x', tempfile())\nfetch()"
        ),
        0
    );
}

#[test]
fn connection_open_uses_argument_matching() {
    // Named formals bind first, so the unnamed `'r'` fills `open`.
    assert_eq!(
        network_warnings("base::url(description = 'https://example.invalid/x', 'r')"),
        1
    );
    assert_eq!(
        network_warnings("curl::curl(url = 'https://example.invalid/x', 'r')"),
        1
    );
    // An unambiguous partial name binds `open`.
    assert_eq!(
        network_warnings("base::url('https://example.invalid/x', op = 'r')"),
        1
    );
    assert_eq!(
        network_warnings("base::socketConnection(host = 'localhost', 80L)"),
        1
    );
    for source in [
        // `o` does not prove an opened connection: it is an ambiguous
        // partial name (`open`/`options`) in current R.
        "base::socketConnection('localhost', port = 80L, o = '')",
        "base::socketConnection('localhost', port = 80L, ope = '')",
        // A computed `open` reached by partial matching stays silent.
        "base::url('https://example.invalid/x', op = mode)",
        "base::url(description = 'https://example.invalid/x', '')",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
}

#[test]
fn wrapper_scan_respects_local_bindings_and_deferred_code() {
    for source in [
        // A function defined locally in the wrapper masks the bare head.
        "fetch <- function() { url <- function(...) NULL; url('https://example.invalid/x', open = 'r') }\nfetch()",
        "fetch <- function() { download.file <- function(u, d) d; download.file('https://example.invalid/x', tempfile()) }\nfetch()",
        // Quoted and delayed code is not executed by calling the wrapper.
        "fetch <- function() quote(utils::download.file('https://example.invalid/x', tempfile()))\nfetch()",
        "fetch <- function() { delayedAssign('x', utils::download.file('https://example.invalid/x', tempfile())) }\nfetch()",
        // A literal-FALSE arm never runs; a literal-TRUE else arm neither.
        "fetch <- function() { if (FALSE) utils::download.file('https://example.invalid/x', tempfile()) }\nfetch()",
        "fetch <- function() { if (TRUE) NULL else utils::download.file('https://example.invalid/x', tempfile()) }\nfetch()",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
    // The executed arm of a literal-TRUE `if` still counts.
    assert_eq!(
        network_warnings(
            "fetch <- function() { if (TRUE) utils::download.file('https://example.invalid/x', tempfile()) }\nfetch()"
        ),
        1
    );
    // A local binding cannot mask a qualified head.
    assert_eq!(
        network_warnings(
            "fetch <- function() { download.file <- function(u, d) d; utils::download.file('https://example.invalid/x', tempfile()) }\nfetch()"
        ),
        1
    );
}

#[test]
fn guard_probe_must_be_established_by_the_condition() {
    for source in [
        // This arm runs specifically in noninteractive sessions.
        "if (identical(interactive(), FALSE)) utils::download.file('https://example.invalid/x', tempfile())",
        "if (identical(Sys.getenv('MYPKG_DOWNLOAD'), '')) utils::download.file('https://example.invalid/x', tempfile())",
        // The probe sits in an unevaluated nested function; the
        // condition's truth says nothing about it.
        "if (length(list(function() interactive())) > 0L) utils::download.file('https://example.invalid/x', tempfile())",
    ] {
        assert_eq!(network_warnings(source), 1, "{source}");
    }
    for source in [
        "if (identical(interactive(), TRUE)) utils::download.file('https://example.invalid/x', tempfile())",
        "if (!identical(interactive(), FALSE)) utils::download.file('https://example.invalid/x', tempfile())",
        "if (Sys.getenv('MYPKG_DOWNLOAD') == '1') utils::download.file('https://example.invalid/x', tempfile())",
        "if (Sys.getenv('MYPKG_DOWNLOAD') != '') utils::download.file('https://example.invalid/x', tempfile())",
        "if (interactive() && isTRUE(getOption('mypkg.download'))) utils::download.file('https://example.invalid/x', tempfile())",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
}

#[test]
fn diverging_guard_requires_base_stop() {
    // A locally defined `stop` returns normally, so the download runs.
    assert_eq!(
        network_warnings(
            ".onLoad <- function(libname, pkgname) { stop <- function(...) NULL; if (!interactive()) stop('skip'); utils::download.file('https://example.invalid/x', tempfile()) }"
        ),
        1
    );
    for source in [
        ".onLoad <- function(libname, pkgname) { if (!interactive()) stop('skip'); utils::download.file('https://example.invalid/x', tempfile()) }",
        // A qualified `base::stop` diverges even beside a local `stop`.
        ".onLoad <- function(libname, pkgname) { stop <- function(...) NULL; if (!interactive()) base::stop('skip'); utils::download.file('https://example.invalid/x', tempfile()) }",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
}

#[test]
fn methods_nested_in_top_level_calls_stay_quiet() {
    // The torch shape: an R6 method body is ordinary function code,
    // however deeply the literal is nested in a top-level call.
    assert_eq!(
        network_warnings(
            "r_session <- R6::R6Class('r_session', public = list(setup = function() { con <- socketConnection(port = 1L, server = TRUE, open = 'a+b') }))"
        ),
        0
    );
    assert_eq!(
        network_warnings(
            "handlers <- list(fetch = function() utils::download.file('https://example.invalid/x', tempfile()))"
        ),
        0
    );
}

#[test]
fn project_wrapper_called_at_top_level_warns_at_the_call() {
    assert_eq!(
        network_warnings(
            "fetch <- function() utils::download.file('https://example.invalid/x', tempfile())\nfetch()"
        ),
        1
    );
    // A guarded wrapper body does not transfer the warning.
    assert_eq!(
        network_warnings(
            "fetch <- function() if (interactive()) utils::download.file('https://example.invalid/x', tempfile())\nfetch()"
        ),
        0
    );
}

#[test]
fn vectorized_and_rhs_still_warns() {
    // `&` evaluates both operands: a network call in its rhs is not
    // guarded by the probe on its lhs. Only `&&` guards the rhs.
    assert_eq!(
        network_warnings(
            "if (getOption('x', FALSE) & utils::download.file('https://example.invalid/x', tempfile())) 1L"
        ),
        1
    );
    assert_eq!(
        network_warnings(
            "if (getOption('x', FALSE) && utils::download.file('https://example.invalid/x', tempfile())) 1L"
        ),
        0
    );
}

#[test]
fn negated_true_while_and_masked_probes() {
    // `while (!TRUE)` is as dead as `while (FALSE)`.
    assert_eq!(
        network_warnings(
            "while (!TRUE) utils::download.file('https://example.invalid/x', tempfile())"
        ),
        0
    );
    // A locally defined `interactive` is not base's opt-in probe.
    assert_eq!(
        network_warnings(
            "interactive <- function() TRUE\nif (interactive()) utils::download.file('https://example.invalid/x', tempfile())"
        ),
        1
    );
    // Nor is a same-named formal inside a load hook.
    assert_eq!(
        network_warnings(
            ".onLoad <- function(libname, pkgname, interactive = function() TRUE) { if (interactive()) utils::download.file('https://example.invalid/x', tempfile()) }"
        ),
        1
    );
    // Qualified `base::interactive()` still guards under a local mask.
    assert_eq!(
        network_warnings(
            "interactive <- function() TRUE\nif (base::interactive()) utils::download.file('https://example.invalid/x', tempfile())"
        ),
        0
    );
}
