# expect: RY033
# `==`/`!=` between character and integer, logical, or integer-valued double
# values compares the formatted strings, which is the intended answer (#699).
# Only the ordering operators compare lexicographically.
in_rstudio <- function() Sys.getenv("RSTUDIO") == 1

has_default_rownames <- function(x) all(rownames(x) == seq_len(nrow(x)))

simplify_result <- function(answer, simplify = TRUE) {
  simplify2array(answer, higher = (simplify == "array"))
}
use <- function(a) simplify_result(a)

not_snow <- function(parallel = FALSE) parallel != "snow"
serial <- function() not_snow()

late_month <- function(day) format(day, "%d") > 14
