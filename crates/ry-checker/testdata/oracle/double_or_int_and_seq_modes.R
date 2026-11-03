# oracle: must-pass
# Premises for the `double_or_int` return rule and `seq()` modes (#866).

# Logical input keeps integer storage, like integer input; any double input
# widens to double.
stopifnot(typeof(sum(c(TRUE, FALSE))) == "integer",
          typeof(sum(1:3)) == "integer",
          typeof(sum(1:3, na.rm = TRUE)) == "integer",
          typeof(abs(TRUE)) == "integer",
          typeof(max(TRUE)) == "integer",
          typeof(cumsum(c(TRUE, FALSE))) == "integer",
          typeof(max(1L, 2.5)) == "double",
          typeof(sum(1.5)) == "double")

# Character and complex input keeps neither mode, and the members disagree.
stopifnot(typeof(max("a", "b")) == "character",
          typeof(min("a")) == "character",
          typeof(range(c("a", "b"))) == "character",
          typeof(sum(complex(real = 1, imaginary = 1))) == "complex",
          typeof(abs(complex(real = 1, imaginary = 1))) == "double")

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
