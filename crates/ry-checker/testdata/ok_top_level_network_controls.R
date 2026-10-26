# no-diag
# Ordinary functions run only when a user calls them.
fetch_season <- function(url) {
  utils::download.file(url, tempfile())
}
# A lazily forced default is not evaluated at load.
with_default <- function(path = utils::download.file("https://example.invalid/x", tempfile())) {
  path
}
# Guarded by an explicit opt-in probe.
if (interactive()) {
  utils::download.file("https://example.invalid/x", tempfile())
}
.onLoad <- function(libname, pkgname) {
  if (!isTRUE(getOption("example.download"))) {
    return(invisible())
  }
  utils::download.file("https://example.invalid/x", tempfile())
}
# Connections R has not opened yet do not touch the network.
pending <- base::url("https://example.invalid/x")
# A local same-named function masks the package head.
download.file <- function(url, dest) dest
download.file("https://example.invalid/x", tempfile())
