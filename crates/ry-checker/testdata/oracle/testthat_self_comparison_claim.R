# oracle: must-pass
# oracle-claim: RY122
# Re-evaluating a deterministic expression on plain data yields the
# identical value, so an equality assertion comparing it to itself
# cannot distinguish two values (the ggplot2 secondary-axis shape,
# where `sec.major` was meant on the right). The differing field and
# the effectful same-text counter show the boundary: same text alone
# is not the premise, deterministic re-evaluation is.
x <- 1.23456
stopifnot(identical(round(x, 3), round(x, 3)))
breaks <- list(major = 1.23456, sec.major = 9.87654)
stopifnot(identical(round(breaks$major, 3), round(breaks$major, 3)))
stopifnot(!identical(round(breaks$major, 3), round(breaks$sec.major, 3)))
counter <- new.env()
counter$n <- 0L
next_value <- function() {
  counter$n <- counter$n + 1L
  counter$n
}
stopifnot(!identical(next_value(), next_value()))
