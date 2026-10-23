# oracle: must-pass
# A top-level rlang-style `on_load({...})` block runs in the namespace when
# `.onLoad()` evaluates it, so its assignments become package bindings that
# functions in other files read (#691). An installed package runs an
# `evalqOnLoad({...})` block in a child of its namespace instead, so `LOG`
# is no package binding. Hooks only run during a namespace load, so this
# script installs a throwaway package with a package-local `on_load()`.
lib <- tempfile()
src <- file.path(tempfile(), "onloadpkg")
dir.create(lib)
dir.create(file.path(src, "R"), recursive = TRUE)
writeLines(c("Package: onloadpkg", "Version: 0.1"), file.path(src, "DESCRIPTION"))
writeLines(c("export(is_local)", "export(log_reset)"), file.path(src, "NAMESPACE"))
writeLines(c(
  "on_load_exprs <- list()",
  "on_load <- function(expr) {",
  "  on_load_exprs[[length(on_load_exprs) + 1]] <<- substitute(expr)",
  "}",
  ".onLoad <- function(libname, pkgname) {",
  "  for (expr in on_load_exprs) eval(expr, envir = topenv())",
  "}"
), file.path(src, "R", "aaa.R"))
writeLines(c(
  "on_load({",
  "  IS_LOCAL_PKG <- exists('.__DEVTOOLS__')",
  "})",
  "evalqOnLoad({",
  "  LOG <- new.env()",
  "})"
), file.path(src, "R", "globals.R"))
writeLines(c(
  "is_local <- function() if (IS_LOCAL_PKG) 'local' else 'installed'",
  "log_reset <- function() rm(list = ls(LOG), envir = LOG)"
), file.path(src, "R", "use.R"))
install.packages(src, lib = lib, repos = NULL, type = "source", quiet = TRUE)
ns <- loadNamespace("onloadpkg", lib.loc = lib)
stopifnot(
  identical(ns$is_local(), "installed"),
  exists("IS_LOCAL_PKG", envir = ns, inherits = FALSE),
  !exists("LOG", envir = ns, inherits = FALSE),
  inherits(try(suppressWarnings(ns$log_reset()), silent = TRUE), "try-error")
)
