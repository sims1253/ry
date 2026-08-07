# no-diag
# Negative controls for the plan 31 W18 recall rules (RY102-RY105). Every
# line here is correct R that a too-broad version of one of those rules
# would flag.

# RY102: `=` names the element; `<-` outside the container family is an
# ordinary assignment and loses nothing.
cfg <- list(ref = "a", `github-ref` = "b", sha = "c")
env <- local(counter <- 0L)

# RY103: `inherits()` is the length-safe test, and `class(x)[1]` is the
# explicit length-1 form. Vectorised comparison outside a scalar logical
# context is exactly what a class vector is for.
is_frame <- function(x) inherits(x, "data.frame")
first_class <- function(x) if (class(x)[1] == "tbl") 1 else 2
matches_any <- function(x) any(class(x) == "tbl")

# RY104: the corrected form puts the comparison inside the reduction, and a
# numeric reduction really does return a number to compare against.
all_present <- function(v) if (any(v == 0)) 1 else 2
none_missing <- function(v) if (sum(v) == 0) 1 else 2

# RY105: an ordinary vector has no length known by construction, and a
# comparison against 1 is a deliberate scalar assertion rather than a dead
# emptiness guard.
non_empty <- function(v) if (length(v) > 0) 1 else 2
is_scalar <- function(v) if (length(sum(v)) == 1) 1 else 2

# RY095 (retired) must stay retired: R parses `!x >= y` as `!(x >= y)`,
# because unary `!` binds *looser* than comparison.
below <- function(x, y) if (!x >= y) 1 else 2
