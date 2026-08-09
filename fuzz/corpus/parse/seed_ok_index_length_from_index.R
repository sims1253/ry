# no-diag
k <- function(x) {
  formats <- c(xlsx = "a", xls = "b", csv = "c", tsv = "d", txt = "e")
  fmt <- unname(formats[x])
  if (is.na(fmt)) stop("bad")
}

chars <- c("a", "b", "c")[c("a", "missing")]
if (length(chars) != 2L) stop("character index length changed")

positive <- c(10L, 20L, 30L)[c(1L, 3L)]
if (length(positive) != 2L) stop("positive numeric index length changed")

scalar <- c(10L, 20L)[1L]
if (length(scalar) != 1L) stop("scalar index length changed")

excluded <- c(10L, 20L)[-1L]
if (length(excluded) > 1L) stop("negative index length is dynamic")

empty <- c(10L, 20L)[0L]
if (length(empty) > 1L) stop("zero index length is dynamic")
