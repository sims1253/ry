# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Maximilian Scholz
#
# Classify R source files by whether GNU R's parser accepts them.
#
# Usage: Rscript --vanilla ecosystem/parse-accept.R <file-list> <output.tsv>
#
# <file-list> holds one path per line. Each output line is
# `path<TAB>ACCEPT` or `path<TAB>REJECT<TAB>first line of R's message`,
# in input order. Files are parsed without evaluation, in one process.

args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 2L) {
  stop("usage: parse-accept.R <file-list> <output.tsv>", call. = FALSE)
}
paths <- readLines(args[[1]], encoding = "UTF-8", warn = FALSE)
paths <- paths[nzchar(paths)]

verdict <- function(path) {
  message <- tryCatch(
    {
      suppressWarnings(parse(file = path, keep.source = FALSE, encoding = "UTF-8"))
      NULL
    },
    error = function(condition) conditionMessage(condition)
  )
  if (is.null(message)) {
    return(paste(path, "ACCEPT", sep = "\t"))
  }
  first <- strsplit(message, "\n", fixed = TRUE)[[1]]
  first <- if (length(first)) trimws(first[[1]]) else ""
  # Keep messages independent of where the corpus is checked out.
  if (startsWith(first, paste0(path, ":"))) {
    first <- substring(first, nchar(path) + 2L)
  }
  paste(path, "REJECT", gsub("[\t\r]", " ", first), sep = "\t")
}

lines <- vapply(paths, verdict, character(1), USE.NAMES = FALSE)
handle <- file(args[[2]], open = "wb")
writeLines(lines, handle, sep = "\n", useBytes = TRUE)
close(handle)
