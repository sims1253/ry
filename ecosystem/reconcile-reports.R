#!/usr/bin/env Rscript
# The production gate and ledger mutation tests use the same entry point.
args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 4L) {
  stop("usage: reconcile-reports.R LEDGER REPORTS PREFIX PACKAGE [PACKAGE ...]")
}
if (any(!nzchar(args[-(1:3)]))) {
  stop("processed package names must not be empty")
}
script <- sub("^--file=", "", grep("^--file=", commandArgs(), value = TRUE))
source(file.path(dirname(normalizePath(script)), "reconcile.R"))
status <- reconcile(args[[1L]], args[[2L]], args[[3L]], args[-(1:3)])
if (status != 0L) quit(status = status)
