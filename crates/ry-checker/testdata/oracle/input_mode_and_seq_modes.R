# oracle: must-pass
# Premises for the `input_mode` return rule (r-typeshed#125) and `seq()`
# modes (#866).

mode_of <- function(f, x) {
  tryCatch(suppressWarnings(typeof(f(x))), error = function(e) "error")
}
inputs <- list(c(TRUE, FALSE), 1:2, c(1.5, 2), complex(real = 1:2, imaginary = 1),
               c("1", "2"))
table <- list(
  sum = c("integer", "integer", "double", "complex", "error"),
  cumsum = c("integer", "integer", "double", "complex", "double"),
  diff = c("integer", "integer", "double", "complex", "error"),
  abs = c("integer", "integer", "double", "double", "error"),
  cummax = c("integer", "integer", "double", "error", "double"),
  cummin = c("integer", "integer", "double", "error", "double"),
  max = c("integer", "integer", "double", "error", "character"),
  min = c("integer", "integer", "double", "error", "character"),
  range = c("integer", "integer", "double", "error", "character"),
  pmax = c("logical", "integer", "double", "error", "character"),
  pmin = c("logical", "integer", "double", "error", "character"),
  rowsum = c("error", "integer", "double", "error", "error")
)
for (name in names(table)) {
  f <- match.fun(name)
  if (name == "rowsum") f <- function(x) rowsum(x, c(1, 1))
  stopifnot(identical(vapply(inputs, mode_of, "", f = f), table[[name]]))
}

# The data join on the coercion ladder; `na.rm` is not data.
stopifnot(typeof(sum(1:3, na.rm = TRUE)) == "integer",
          typeof(max(1L, 2.5)) == "double",
          typeof(max(2.5, 1L)) == "double",
          typeof(max(TRUE, "a")) == "character",
          typeof(sum(TRUE, complex(real = 1, imaginary = 1))) == "complex",
          typeof(sum(1+1i)) == "complex",
          typeof(pmax(TRUE, 2L)) == "integer",
          typeof(max(1.5, "a")) == "character")

# pmax and pmin of logical data depend on the argument count, so the rule
# leaves logical out of their table.
stopifnot(typeof(pmax(c(TRUE, FALSE))) == "logical",
          typeof(pmax(TRUE, FALSE)) == "integer",
          typeof(pmin(TRUE, FALSE)) == "integer")

# Empty data makes max, min and range double (-Inf/Inf), unless the data
# are character; other members keep integer.
stopifnot(typeof(suppressWarnings(max(integer(0)))) == "double",
          typeof(suppressWarnings(range(logical(0)))) == "double",
          typeof(suppressWarnings(max())) == "double",
          typeof(suppressWarnings(max(seq(length.out = 0)))) == "double",
          typeof(suppressWarnings(min(seq_len(0)))) == "double",
          typeof(suppressWarnings(max(character(0)))) == "character",
          typeof(max(integer(0), 1L)) == "integer",
          typeof(sum(integer(0))) == "integer",
          typeof(max(c(TRUE, NA), na.rm = TRUE)) == "integer",
          typeof(suppressWarnings(max(NA, na.rm = TRUE))) == "double",
          typeof(max(1:2, na.rm = FALSE)) == "integer")

# seq(from, to) is from:to; a `by` step keeps integer storage only when
# from, to and by are all integer or logical.
stopifnot(typeof(seq(1, 10)) == "integer",
          typeof(seq(1.5, 10)) == "double",
          typeof(seq(10)) == "integer",
          typeof(seq(1, 10, by = 2)) == "double",
          typeof(seq(1, 10, by = 1L)) == "double",
          typeof(seq(1L, 10L, by = 2L)) == "integer",
          typeof(seq(1, 10, length.out = 4)) == "double",
          typeof(seq(length.out = 3)) == "integer")

# seq.int differs: whole doubles keep integer storage, so its `by` and
# `length.out` forms depend on values.
stopifnot(typeof(seq.int(1, 10, by = 1)) == "integer",
          typeof(seq.int(1, 10, by = 1.5)) == "double",
          typeof(seq.int(1, 10, length.out = 4)) == "integer",
          typeof(seq.int(1, 10, length.out = 3)) == "double")

# A partial `length.out` name is still `length.out`.
stopifnot(length(seq(1, 10, length = 4)) == 4L,
          typeof(seq(1, 10, len = 4)) == "double")
