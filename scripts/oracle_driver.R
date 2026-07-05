#!/usr/bin/env Rscript

# oracle_driver.R -- evaluate every oracle fixture in parallel via
# purrr::map + mirai::in_parallel, emitting one JSON object per fixture.
#
# This is the dogfood path for PLAN Phase 3.3: the oracle suite (which
# spawns one Rscript --vanilla per fixture serially, ~8s wall for ~50
# fixtures) is replaced by a SINGLE Rscript invocation that sets up
# mirai daemons and evaluates each fixture through purrr's parallel map.
# The Rust oracle harness parses this stdout and applies the existing
# must-flag / must-pass / known-gap logic unchanged.
#
# Each fixture is evaluated in a FRESH environment (new.env(parent =
# baseenv())) so top-level assignments do not leak. ISOLATION CAVEAT:
# mirai daemons persist across fixtures, so a fixture that calls
# library() / attaches a package / writes a global WILL leak state to
# later fixtures run on the same daemon. The oracle fixtures are kept
# side-effect-free on purpose; if you add a fixture that attaches a
# package, the parallel driver is the wrong path -- fall back to the
# serial per-fixture Rscript path (the Rust harness probes for purrr/
# mirai and falls back automatically).
#
# Usage (invoked by crates/ry-checker/tests/oracle.rs):
#   Rscript scripts/oracle_driver.R <fixture_dir>
#
# Output: one JSON object per line on stdout:
#   {"file":"arith_character.R","errored":true,"message":"..."}
#   {"file":"if_numeric.R","errored":false,"message":""}
#
# The driver exits 0 if every fixture was evaluated (regardless of
# whether R errored on any individual fixture), and nonzero only if the
# driver itself failed to set up.

main <- function() {
  args <- commandArgs(trailingOnly = TRUE)
  if (length(args) < 1) {
    message("Usage: Rscript scripts/oracle_driver.R <fixture_dir>")
    quit(status = 2)
  }
  fixture_dir <- args[[1]]
  if (!dir.exists(fixture_dir)) {
    message(sprintf("oracle_driver: %s does not exist", fixture_dir))
    quit(status = 2)
  }
  # carrier is purrr's SUGGESTED dependency that in_parallel() requires
  # at call time (carrier::crate); without it the driver's own map()
  # call below errors even though purrr itself loads fine.
  if (!requireNamespace("purrr", quietly = TRUE) ||
      !requireNamespace("mirai", quietly = TRUE) ||
      !requireNamespace("carrier", quietly = TRUE)) {
    message("oracle_driver: purrr, mirai, and/or carrier not installed; the harness should fall back to the serial path")
    quit(status = 3)
  }

  files <- list.files(fixture_dir, pattern = "\\.R$", full.names = TRUE)
  if (length(files) == 0) {
    message("oracle_driver: no .R fixtures in ", fixture_dir)
    quit(status = 0)
  }
  names(files) <- basename(files)

  # Spin up daemons sized to the fixture count (capped to available
  # cores). Daemons are torn down on exit. Only the side-effect-free
  # fixtures (below) actually use them; the rest fall back to a serial
  # subprocess.
  #
  # Use base R's parallel::detectCores() rather than
  # parallelly::availableCores(): `parallel` ships with R (no extra
  # dependency, so no requireNamespace guard needed), whereas `parallelly`
  # is not guaranteed installed -- an unguarded call to it killed the
  # driver on machines without it, silently dropping the parallel path.
  # detectCores() can return NA (unknown core count), so fall back to 2L.
  cores <- parallel::detectCores()
  if (is.na(cores)) cores <- 2L
  n <- min(length(files), max(1L, cores - 1L))
  mirai::daemons(n)
  on.exit(mirai::daemons(0), add = TRUE)

  # Evaluate a single fixture, capturing whether R errored. ISOLATION:
  # mirai daemons persist across fixtures, so a fixture that calls
  # mirai::daemons() (nested-daemon error), uses <<- (writes a shared
  # global), or library() (attaches to a shared search path) CANNOT be
  # isolated inside a daemon. Detect those side-effecting fixtures by a
  # source scan and fall back to a fresh `Rscript --vanilla` subprocess
  # for them (the serial path, fully isolated). Side-effect-free
  # fixtures (the majority) go through purrr::in_parallel + mirai.
  is_isolated <- function(path) {
    src <- tryCatch(readLines(path), error = function(e) character(0))
    if (length(src) == 0) return(FALSE)
    # Strip comments before scanning (a comment mentioning daemons does
    # not actually call it).
    code <- sub("#.*$", "", src)
    body <- paste(code, collapse = "\n")
    !grepl("daemons\\(", body, fixed = FALSE) &&
      !grepl("<<-", body, fixed = TRUE) &&
      !grepl("\\blibrary\\(", body, perl = TRUE) &&
      !grepl("\\brequire\\(", body, perl = TRUE)
  }
  parallel_ok <- vapply(files, is_isolated, logical(1))

  # Parallel-evaluable fixtures: dispatch to mirai daemons via
  # purrr::in_parallel. Each is parsed+evaluated in a fresh environment
  # so top-level assignments do not leak; parsed so a syntax error is
  # reported as an error (matching Rscript's behaviour).
  eval_one <- purrr::in_parallel(function(path) {
    out <- list(file = basename(path), errored = FALSE, message = "")
    tryCatch(
      {
        exprs <- parse(path)
        envir <- new.env(parent = baseenv())
        for (e in exprs) eval(e, envir = envir)
      },
      error = function(e) {
        out$errored <<- TRUE
        out$message <<- conditionMessage(e)
      }
    )
    out
  })
  results <- if (any(parallel_ok)) {
    purrr::map(files[parallel_ok], eval_one)
  } else {
    list()
  }

  # Side-effecting fixtures: a fresh `Rscript --vanilla` subprocess each
  # (the serial path). system2 captures the exit code; a nonzero exit OR
  # "Error" on stderr counts as errored.
  serial_results <- vector("list", sum(!parallel_ok))
  serial_files <- files[!parallel_ok]
  for (i in seq_along(serial_files)) {
    path <- serial_files[[i]]
    out <- list(file = basename(path), errored = FALSE, message = "")
    res <- system2(
      file.path(R.home("bin"), "Rscript"),
      args = c("--vanilla", path),
      stdout = TRUE, stderr = TRUE
    )
    attr <- attributes(res)
    status <- attr[["status"]]
    combined <- paste(as.character(res), collapse = "\n")
    if (!is.null(status) && status != 0) {
      out$errored <- TRUE
      out$message <- "nonzero exit"
    } else if (grepl("Error", combined, fixed = TRUE)) {
      out$errored <- TRUE
      out$message <- "Error on stderr/stdout"
    }
    serial_results[[i]] <- out
  }

  for (r in c(results, serial_results)) {
    cat(to_json_line(r), "\n", sep = "")
  }
}

# Hand-serialize one result object to a single JSON line (no jsonlite
# dependency; the shape is fixed and simple). Strings are escaped.
to_json_line <- function(x) {
  sprintf(
    '{"file":"%s","errored":%s,"message":"%s"}',
    escape_json(x$file),
    tolower(x$errored),
    escape_json(x$message)
  )
}

escape_json <- function(s) {
  s <- gsub("\\", "\\\\", s, fixed = TRUE)
  s <- gsub('"', '\\"', s, fixed = TRUE)
  s <- gsub("\n", "\\n", s, fixed = TRUE)
  s <- gsub("\r", "\\r", s, fixed = TRUE)
  s <- gsub("\t", "\\t", s, fixed = TRUE)
  s
}

main()
