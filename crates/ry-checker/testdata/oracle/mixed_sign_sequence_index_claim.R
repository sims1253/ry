# oracle: must-pass
# oracle-claim: RY126
# A whole-number sequence whose endpoints straddle zero contains
# both a positive and a negative term, and base `[` rejects such an
# index on plain vectors and lists ("only 0's may be mixed with
# negative subscripts"), for reads and assignments alike. The
# flagged shapes are evaluated from strings so this claim file
# itself checks clean; the boundary shapes run directly.
x <- 1:10
mixed_error <- function(text) {
  err <- tryCatch(eval(parse(text = text)), error = identity)
  inherits(err, "error") &&
    grepl("only 0's may be mixed with negative subscripts", conditionMessage(err), fixed = TRUE)
}
stopifnot(mixed_error("x[1:-1]"))
stopifnot(mixed_error("x[seq(1, -1)]"))
stopifnot(mixed_error("x[seq.int(2, -3)]"))
stopifnot(mixed_error("x[1:-1] <- 0L"))
err <- tryCatch(eval(parse(text = "x[[1:-1]]")), error = identity)
stopifnot(inherits(err, "error"))
if (requireNamespace("rlang", quietly = TRUE)) {
  # rlang::seq2() ascends or is empty: seq2(-1, 2) is -1:2, while
  # seq2(1, -1) is integer(0), not a descending sequence.
  stopifnot(mixed_error("x[rlang::seq2(-1, 2)]"))
  stopifnot(identical(rlang::seq2(1, -1), integer()))
  stopifnot(identical(x[rlang::seq2(1, -1)], integer()))
}
# Descending one-sided sequences are legitimate subscripts, and a
# `by` that lands exactly on 0 never reaches a negative term.
stopifnot(identical(x[3:1], c(3L, 2L, 1L)))
stopifnot(identical(x[-3:-1], x[c(4L, 5L, 6L, 7L, 8L, 9L, 10L)]))
stopifnot(identical(seq(2, -1, by = -2), c(2, 0)))
stopifnot(identical(x[seq(2, -1, by = -2)], x[2L]))
