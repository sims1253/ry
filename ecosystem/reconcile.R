# Reconciliation logic shared by ecosystem/run.sh and test-reconciliation.R.
#
# Compares expected diagnostic identities from a corpus ledger against actual
# diagnostics from hermetic reports, and returns the exit status (0 = pass,
# 1 = fail) according to the reconciliation mode and finding labels.

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

#' Read actual diagnostic identities from a .root.txt report.
read_actual_identities <- function(reports_dir, audited) {
  actual <- character(0)
  for (package in audited) {
    report <- file.path(reports_dir, paste0(package, ".root.txt"))
    lines <- readLines(report, encoding = "UTF-8", warn = FALSE)
    lines <- lines[nzchar(lines)]
    for (entry in lines) {
      fields <- regmatches(entry, regexec("^(.*):([0-9]+):([0-9]+) (RY[0-9]+)$", entry))[[1]]
      if (length(fields) != 5L) stop("malformed ecosystem report entry: ", entry)
      actual <- c(actual, identity_key(package, fields[[5]], fields[[2]], fields[[3]], fields[[4]]))
    }
  }
  actual
}

#' Reconcile actual reports against a corpus ledger.
#'
#' @param corpus_path Path to the corpus JSON.
#' @param reports_dir Directory containing the .root.txt report files.
#' @param processed Character vector of package names that were processed.
#' @return Integer exit status: 0 = pass, 1 = fail.
reconcile <- function(corpus_path, reports_dir, processed) {
  corpus <- jsonlite::fromJSON(corpus_path, simplifyDataFrame = TRUE)
  audited <- intersect(processed, corpus$packages$name)

  expected_rows <- corpus$findings[corpus$findings$package %in% audited, , drop = FALSE]
  expected <- identity_key(
    expected_rows$package,
    expected_rows$code,
    expected_rows$path,
    expected_rows$line,
    expected_rows$column
  )
  actual <- read_actual_identities(reports_dir, audited)

  missing <- multiset_delta(expected, actual)
  unowned <- multiset_delta(actual, expected)
  required_rows <- expected_rows[
    which(expected_rows$label == "true_positive"),
    ,
    drop = FALSE
  ]
  required <- identity_key(
    required_rows$package,
    required_rows$code,
    required_rows$path,
    required_rows$line,
    required_rows$column
  )
  missing_required <- multiset_delta(required, actual)

  if (length(missing_required)) {
    writeLines(c("ecosystem: required reviewed findings disappeared:", paste0("  - ", missing_required)), stderr())
  }
  other_missing <- multiset_delta(missing, missing_required)
  if (length(other_missing)) {
    writeLines(c("ecosystem: reviewed audit findings disappeared:", paste0("  - ", other_missing)), stderr())
  }
  if (length(unowned)) {
    writeLines(c("ecosystem: unowned hermetic findings appeared:", paste0("  - ", unowned)), stderr())
  }
  if (length(missing) || length(unowned)) {
    if (length(missing_required)) {
      writeLines("ecosystem: a reviewed true_positive finding disappeared; this gates the build in every mode.", stderr())
      return(1L)
    }
    reconciliation_mode <- if (!is.null(corpus$reconciliation) && length(corpus$reconciliation) > 0) corpus$reconciliation[[1L]] else "hermetic"
    if (reconciliation_mode == "audit-transcript") {
      writeLines(c(
        sprintf("ecosystem: %s is an audit transcript of an installed-library run;", corpus_path),
        "ecosystem: the hermetic-vs-audit delta above is reported for visibility and does not gate this build.",
        sprintf("ecosystem: to update the ledger, re-audit the %s corpus and regenerate it from the audit results (see docs/corpus/README.md).", corpus$corpus[[1L]])
      ), stderr())
      return(0L)
    } else {
      writeLines(sprintf("ecosystem: update %s with the reviewed workstream delta", corpus_path), stderr())
      return(1L)
    }
  }
  0L
}
