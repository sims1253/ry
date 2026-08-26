#!/usr/bin/env Rscript
# Integration test: the ecosystem reconciliation gating logic.
#
# Tests the SAME reconcile() function that ecosystem/run.sh calls, sourced
# from ecosystem/reconcile.R. Each test case builds minimal corpus + report
# fixtures and asserts the exit status for each combination of finding status,
# reconciliation mode, and delta direction.

suppressPackageStartupChars <- function(x) x
library(jsonlite)

repo_root <- normalizePath(Sys.getenv("RY_REPO_ROOT", "."), mustWork = TRUE)
source(file.path(repo_root, "ecosystem", "reconcile.R"))

tmp <- tempfile()
dir.create(tmp)
on.exit(unlink(tmp, recursive = TRUE), add = TRUE)

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
  reconcile(corpus_path, tmp, "pkg")
}

tp <- list(package = "pkg", code = "RY010", path = "R/a.R", line = 1, column = 1, label = "true_positive", workstream = "w1")
fp <- list(package = "pkg", code = "RY033", path = "R/b.R", line = 2, column = 3, label = "false_positive", workstream = "w2")

cat("Test 1: clean tree (all findings present) -> exit 0\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp)), list(pkg = list(tp, fp))) == 0L)
cat("  PASS\n")

cat("Test 2: true_positive disappears in hermetic mode -> exit 1\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp)), list(pkg = list(fp))) == 1L)
cat("  PASS\n")

cat("Test 3: true_positive disappears in audit-transcript mode -> exit 1\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp), "audit-transcript"), list(pkg = list(fp))) == 1L)
cat("  PASS\n")

cat("Test 4: false_positive disappears in hermetic mode -> exit 1\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp)), list(pkg = list(tp))) == 1L)
cat("  PASS\n")

cat("Test 5: false_positive disappears in audit-transcript mode -> exit 0\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp), "audit-transcript"), list(pkg = list(tp))) == 0L)
cat("  PASS\n")

cat("Test 6: unowned finding appears in hermetic mode -> exit 1\n")
extra <- list(package = "pkg", code = "RY040", path = "R/c.R", line = 5, column = 1, label = "false_positive", workstream = "w3")
stopifnot(run_reconcile(make_corpus(list(tp, fp)), list(pkg = list(tp, fp, extra))) == 1L)
cat("  PASS\n")

cat("Test 7: unowned finding appears in audit-transcript mode -> exit 0\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp), "audit-transcript"), list(pkg = list(tp, fp, extra))) == 0L)
cat("  PASS\n")

# simultaneous TP disappearance and FP appearance (the exact
# silent-degradation scenario the gate exists to catch). A true_positive disappears
# from reports AND a false_positive appears that is not in the ledger, both
# in the same reconciliation run. The gate must catch both in one pass.
cat("Test 8: TP disappears AND unowned FP appears simultaneously (hermetic) -> exit 1\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp)), list(pkg = list(fp, extra))) == 1L)
cat("  PASS\n")

cat("Test 9: TP disappears AND unowned FP appears simultaneously (audit-transcript) -> exit 1\n")
stopifnot(run_reconcile(make_corpus(list(tp, fp), "audit-transcript"), list(pkg = list(fp, extra))) == 1L)
cat("  PASS\n")

cat("\nAll reconciliation tests passed.\n")
