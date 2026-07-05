#!/usr/bin/env Rscript

# gen_typeshed.R -- emit a DRAFT typeshed JSON for a package.
#
# Hand-writing JSON does not scale past base R. This script takes a
# package name, enumerates its exported functions, and emits one entry
# per function with the parameter list (from `formals()`) and a return
# type of "unknown" (opaque) for a human to refine. It is a CURATION
# AID, not an oracle: it does not infer return types, S3 classes, or
# column schemas, and it cannot distinguish functions that need arg-
# slot modeling (e.g. "arg0") from those that genuinely return opaque.
# Treat its output as a skeleton to edit, then run audit_typeshed.R to
# confirm no fabricated names slipped in.
#
# Usage:
#   Rscript scripts/gen_typeshed.R <package>            # prints to stdout
#   Rscript scripts/gen_typeshed.R <package> > out.json # redirect to file
#
# The package must be installed in the current R library; the script
# attaches it in a child namespace (via::) to avoid polluting the
# search path, then lists exports with getNamespaceExports().

main <- function(argv) {
  pkg <- argv[[1]]
  if (is.na(pkg) || pkg == "--help" || pkg == "-h") {
    usage()
    return(invisible())
  }
  if (!requireNamespace(pkg, quietly = TRUE)) {
    message(sprintf("Package '%s' is not installed; install it first.", pkg))
    quit(status = 1)
  }
  ns <- getNamespace(pkg)
  exports <- getNamespaceExports(ns)
  funs <- sort(exports[vapply(exports, function(n) {
    exists(n, where = ns, mode = "function", inherits = FALSE)
  }, logical(1))])
  cat(to_json_draft(pkg, funs, ns), "\n")
  message(sprintf("# Generated %d function entries for '%s'.\n",
                  length(funs), pkg))
}

# Emit a JSON draft. We hand-serialize (no jsonlite dependency) since
# the shape is simple and we want stable key ordering.
to_json_draft <- function(pkg, funs, ns) {
  lines <- character(0)
  lines <- c(lines, sprintf('  "version": "draft",'))
  lines <- c(lines, '  "functions": {')
  entries <- vapply(funs, function(name) {
    fm <- tryCatch(formals(get(name, envir = ns)), error = function(e) NULL)
    params <- if (is.null(fm)) character(0) else names(fm)
    # Drop the "..." param hint duplicates / empty; keep order.
    params <- params[nzchar(params)]
    param_json <- if (length(params) == 0) {
      "[]"
    } else {
      paste0("[", paste(sprintf('"%s"', escape(name = params)), collapse = ", "), "]")
    }
    sprintf(
      '    "%s": {\n      "params": %s,\n      "return": { "mode": "opaque", "length": "unknown", "na": false }\n    }',
      name, param_json
    )
  }, character(1))
  lines <- c(lines, paste(entries, collapse = ",\n"))
  lines <- c(lines, "  }")
  paste0("{\n", paste(lines, collapse = "\n"), "\n}")
}

escape <- function(name) {
  gsub('"', '\\"', name, fixed = TRUE)
}

usage <- function() {
  cat("Usage: Rscript scripts/gen_typeshed.R <package>\n",
      "\n",
      "Emits a DRAFT typeshed JSON skeleton for <package> to stdout.\n",
      "Each exported function gets its formals() params and an opaque\n",
      "return type for a human to refine. Not an oracle: curation aid only.\n",
      sep = "")
}

main(commandArgs(trailingOnly = TRUE))
