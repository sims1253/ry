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
        "if (interactive()) utils::download.file('https://example.invalid/x', tempfile())",
        "if (isTRUE(getOption('mypkg.download'))) utils::download.file('https://example.invalid/x', tempfile())",
        "if (nzchar(Sys.getenv('MYPKG_DOWNLOAD'))) utils::download.file('https://example.invalid/x', tempfile())",
        ".onLoad <- function(libname, pkgname) { if (!interactive()) return(invisible()); utils::download.file('https://example.invalid/x', tempfile()) }",
    ] {
        assert_eq!(network_warnings(source), 0, "{source}");
    }
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
