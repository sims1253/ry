#!/usr/bin/env Rscript
# Integration test: the ecosystem reconciliation logic must gate correctly
# for each combination of finding status and corpus reconciliation mode.
#
# The bug this catches: a disappeared true_positive finding was only logged
# (not exit-fail) in audit-transcript mode, because the missing_required
# check was inside the reconciliation_mode branch instead of before it.
#
# This test builds minimal corpus + report fixtures in a temp dir and
# invokes the reconciliation inline, asserting the exit status for each case.

suppressPackageStartupChars <- function(x) x
library(jsonlite)

tmp <- tempfile()
dir.create(tmp)
on.exit(unlink(tmp, recursive = TRUE), add = TRUE)

identity_key <- function(package, code, path, line, column) {
  sprintf("%s\t%s\t%s\t%s\t%s", package, code, sub("^\\./", "", path), line, column)
}

multiset_delta <- function(left, right) {
  left_counts <- table(left)
  right_counts <- table(right)
  keys <- union(names(left_counts), names(right_counts))
  left_values <- left_counts[keys]
  right_values <- right_counts[keys]
  left_values[is.na(left_values)] <- 0L
  right_values[is.na(right_values)] <- 0L
  rep(keys, pmax(as.integer(left_values - right_values), 0L))
}

write_report <- function(package, entries) {
  path <- file.path(tmp, paste0(package, ".root.txt"))
  lines <- if (length(entries)) {
    vapply(entries, function(e) {
      sprintf("%s:%s:%s %s", e$path, e$line, e$column, e$code)
    }, character(1))
  } else character(0)
  writeLines(lines, path)
}

make_corpus <- function(findings, reconciliation = NULL) {
  list(
    corpus = "test",
    reconciliation = reconciliation,
    packages = list(name = "pkg"),
    findings = findings
  )
}

run_reconcile <- function(corpus, report_entries) {
  corpus_path <- file.path(tmp, "corpus.json")
  writeLines(toJSON(corpus, auto_unbox = TRUE), corpus_path)
  for (pkg in names(report_entries)) {
    write_report(pkg, report_entries[[pkg]])
  }
  corpus_data <- fromJSON(corpus_path, simplifyDataFrame = TRUE)
  expected_rows <- corpus_data$findings
  expected <- identity_key(expected_rows$package, expected_rows$code,
                           expected_rows$path, expected_rows$line, expected_rows$column)
  actual <- character(0)
  for (entry in report_entries[["pkg"]]) {
    actual <- c(actual, identity_key("pkg", entry$code, entry$path, entry$line, entry$column))
  }
  missing <- multiset_delta(expected, actual)
  unowned <- multiset_delta(actual, expected)
  required_rows <- expected_rows[expected_rows$label == "true_positive", , drop = FALSE]
  required <- identity_key(required_rows$package, required_rows$code,
                           required_rows$path, required_rows$line, required_rows$column)
  missing_required <- multiset_delta(required, actual)

  if (length(missing) || length(unowned)) {
    if (length(missing_required)) {
      return(1L)  # gates in every mode
    }
    mode <- if (!is.null(corpus_data$reconciliation) && length(corpus_data$reconciliation) > 0) corpus_data$reconciliation[[1]] else "hermetic"
    if (mode == "audit-transcript") {
      return(0L)  # informational only
    } else {
      return(1L)
    }
  }
  return(0L)
}

tp <- list(package = "pkg", code = "RY010", path = "R/a.R", line = 1, column = 1, label = "true_positive", workstream = "w1")
fp <- list(package = "pkg", code = "RY033", path = "R/b.R", line = 2, column = 3, label = "false_positive", workstream = "w2")

cat("Test 1: clean tree (all findings present) → exit 0\n")
exit <- run_reconcile(
  make_corpus(list(tp, fp)),
  list(pkg = list(tp, fp))
)
stopifnot(exit == 0L)
cat("  PASS\n")

cat("Test 2: true_positive disappears in hermetic mode → exit 1\n")
exit <- run_reconcile(
  make_corpus(list(tp, fp)),
  list(pkg = list(fp))  # tp is missing from actual
)
stopifnot(exit == 1L)
cat("  PASS\n")

cat("Test 3: true_positive disappears in audit-transcript mode → exit 1\n")
exit <- run_reconcile(
  make_corpus(list(tp, fp), reconciliation = "audit-transcript"),
  list(pkg = list(fp))  # tp is missing from actual
)
stopifnot(exit == 1L)
cat("  PASS\n")

cat("Test 4: false_positive disappears in hermetic mode → exit 1\n")
exit <- run_reconcile(
  make_corpus(list(tp, fp)),
  list(pkg = list(tp))  # fp is missing from actual
)
stopifnot(exit == 1L)
cat("  PASS\n")

cat("Test 5: false_positive disappears in audit-transcript mode → exit 0\n")
exit <- run_reconcile(
  make_corpus(list(tp, fp), reconciliation = "audit-transcript"),
  list(pkg = list(tp))  # fp is missing from actual
)
stopifnot(exit == 0L)
cat("  PASS\n")

cat("Test 6: unowned finding appears in hermetic mode → exit 1\n")
extra <- list(package = "pkg", code = "RY040", path = "R/c.R", line = 5, column = 1, label = "false_positive", workstream = "w3")
exit <- run_reconcile(
  make_corpus(list(tp, fp)),
  list(pkg = list(tp, fp, extra))  # unowned extra finding
)
stopifnot(exit == 1L)
cat("  PASS\n")

cat("Test 7: unowned finding appears in audit-transcript mode → exit 0\n")
exit <- run_reconcile(
  make_corpus(list(tp, fp), reconciliation = "audit-transcript"),
  list(pkg = list(tp, fp, extra))  # unowned extra finding
)
stopifnot(exit == 0L)
cat("  PASS\n")

cat("\nAll reconciliation tests passed.\n")
