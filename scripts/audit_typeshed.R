# Audit typeshed for fabricated entries.
#
# Walks every function-name key in each typeshed data file and asserts it
# is a real R object, catching hallucinated typeshed entries:
#   * base.json  -- names must resolve in a vanilla R session (search
#                   path, or a registered S3 method).
#   * dplyr.json / purrr.json / mirai.json -- names must exist in the
#                   package's namespace. If the package is not installed,
#                   the file is skipped (not failed) so the audit stays
#                   green on minimal machines; CI can install the packages.
#   * bayes.json -- keys are `<pkg>.<function>`; the first dot-segment is
#                   the package. Each function is checked in its package
#                   namespace, skipping uninstalled packages.
#
# This script is base-R only: it does not depend on jsonlite (or any
# other package) and never touches the network. We only need the
# top-level function-name keys of each "functions" object, which we
# extract with a line-based regex instead of parsing the full JSON.
# Namespace introspection uses base's requireNamespace / asNamespace.
#
# Run: Rscript scripts/audit_typeshed.R

# Locate the typeshed data relative to this script so the audit works
# regardless of the caller's working directory.
script_dir <- tryCatch({
  args <- commandArgs(trailingOnly = FALSE)
  file_arg <- grep("^--file=", args, value = TRUE)
  if (length(file_arg) == 1) {
    normalizePath(dirname(sub("^--file=", "", file_arg[1])))
  } else {
    "."
  }
}, error = function(e) ".")

data_dir <- file.path(script_dir, "..", "crates", "ry-typeshed", "data")
data_dir <- normalizePath(data_dir, mustWork = TRUE)

# Extract the function-name keys from a typeshed JSON file. The structure
# is:
#   {
#     "version": "0.0.1",
#     "functions": {
#       "c": { ... },
#       "length": { ... },
#       ...
#     }
#   }
# Function names are the quoted keys at indent level 2 (4 spaces) inside
# the "functions" object, each followed by ": {".
extract_names <- function(data_path) {
  lines <- readLines(data_path)

  func_start <- grep('^  "functions": \\{', lines)
  if (length(func_start) != 1L) {
    stop("Could not locate a single 'functions' object in ", data_path)
  }

  name_pattern <- '^    "([^"]+)": \\{'
  close_pattern <- '^  \\}'

  names_all <- character(0)
  for (i in seq.int(func_start[1L] + 1L, length(lines))) {
    line <- lines[i]
    if (grepl(close_pattern, line, perl = TRUE)) {
      break
    }
    m <- regmatches(line, regexec(name_pattern, line))[[1]]
    if (length(m) >= 2L) {
      names_all <- c(names_all, m[2])
    }
  }
  names_all
}

failures <- character(0)

# ---------------------------------------------------------------------
# base.json -- vanilla R search path / S3 methods.
# ---------------------------------------------------------------------

# A name resolves if it is directly visible on the search path, or (for
# names of the form "<generic>.<class>") it is a registered S3 method
# resolvable via getS3method. The latter are real base-R functions even
# though they are not exported onto search(). Split on the LAST dot,
# since class names can themselves contain dots.
s3_pattern <- "^[A-Za-z.][A-Za-z0-9._]*\\.[A-Za-z.][A-Za-z0-9.]*$"
name_exists <- function(name) {
  if (exists(name, where = search())) {
    return(TRUE)
  }
  if (grepl(s3_pattern, name, perl = TRUE)) {
    all_dots <- gregexpr("\\.", name, perl = TRUE)[[1]]
    dot_pos <- all_dots[length(all_dots)]
    generic <- substr(name, 1, dot_pos - 1)
    klass <- substr(name, dot_pos + 1, nchar(name))
    m <- tryCatch(
      getS3method(generic, klass, optional = TRUE),
      error = function(e) NULL
    )
    if (!is.null(m)) return(TRUE)
  }
  FALSE
}

base_path <- file.path(data_dir, "base.json")
base_names <- extract_names(base_path)
if (length(base_names) == 0L) {
  stop("No function names extracted from ", base_path)
}
cat(sprintf("Auditing %d names from base.json\n", length(base_names)))
for (name in base_names) {
  if (name_exists(name)) {
    cat(sprintf("PASS: %s\n", name))
  } else {
    cat(sprintf("FAIL: %s (not found in a vanilla R session)\n", name))
    failures <- c(failures, sprintf("base:%s", name))
  }
}

# ---------------------------------------------------------------------
# Single-package stubs -- names must exist in the package namespace.
# Skip (do not fail) when the package is not installed.
# ---------------------------------------------------------------------
audit_package_file <- function(file, pkg) {
  path <- file.path(data_dir, file)
  if (!file.exists(path)) {
    cat(sprintf("SKIP: %s not present; nothing to audit\n", file))
    return(character(0))
  }
  names_all <- extract_names(path)
  if (!requireNamespace(pkg, quietly = TRUE)) {
    cat(sprintf(
      "SKIP: package %s not installed; skipping %d names from %s\n",
      pkg, length(names_all), file
    ))
    return(character(0))
  }
  ns <- asNamespace(pkg)
  cat(sprintf(
    "Auditing %d names from %s (package %s)\n",
    length(names_all), file, pkg
  ))
  fails <- character(0)
  for (name in names_all) {
    if (exists(name, envir = ns)) {
      cat(sprintf("PASS: %s::%s\n", pkg, name))
    } else {
      cat(sprintf(
        "FAIL: %s::%s (not defined in package %s)\n", pkg, name, pkg
      ))
      fails <- c(fails, sprintf("%s::%s", pkg, name))
    }
  }
  fails
}

failures <- c(failures, audit_package_file("dplyr.json", "dplyr"))
failures <- c(failures, audit_package_file("purrr.json", "purrr"))
failures <- c(failures, audit_package_file("mirai.json", "mirai"))

# ---------------------------------------------------------------------
# bayes.json -- keys are `<pkg>.<function>`; the first dot-segment is the
# package name. Check each function in its package namespace, skipping
# uninstalled packages (brms, posterior, ... may not be present here).
# ---------------------------------------------------------------------
bayes_path <- file.path(data_dir, "bayes.json")
if (file.exists(bayes_path)) {
  bayes_names <- extract_names(bayes_path)
  cat(sprintf("Auditing %d names from bayes.json\n", length(bayes_names)))
  # Cache the requireNamespace result per package so we only probe once.
  pkg_installed <- new.env(parent = emptyenv())
  for (key in bayes_names) {
    dot <- regexpr(".", key, fixed = TRUE)
    if (dot < 1L) {
      cat(sprintf("FAIL: %s (bayes.json key has no package prefix)\n", key))
      failures <- c(failures, sprintf("bayes:%s", key))
      next
    }
    pkg <- substr(key, 1L, dot - 1L)
    func <- substr(key, dot + 1L, nchar(key))
    if (is.null(pkg_installed[[pkg]])) {
      pkg_installed[[pkg]] <- requireNamespace(pkg, quietly = TRUE)
    }
    if (!isTRUE(pkg_installed[[pkg]])) {
      cat(sprintf("SKIP: package %s not installed; skipping %s\n", pkg, key))
      next
    }
    if (exists(func, envir = asNamespace(pkg))) {
      cat(sprintf("PASS: %s (%s::%s)\n", key, pkg, func))
    } else {
      cat(sprintf(
        "FAIL: %s (%s not defined in package %s)\n", key, func, pkg
      ))
      failures <- c(failures, sprintf("bayes:%s", key))
    }
  }
}

cat("\n")
if (length(failures) == 0) {
  cat("All names verified: no fabricated entries.\n")
  quit(status = 0)
} else {
  cat(sprintf("FAIL: %d fabricated name(s) found:\n", length(failures)))
  for (f in failures) cat(sprintf("  - %s\n", f))
  quit(status = 1)
}
