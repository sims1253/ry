# expect: RY001
# The journal's continuation view stays return-blind (DivergenceView in
# infer/mod.rs): `if (is.null(x)) return(NULL)` must keep `x`'s stale
# NULL-default binding rather than flow the else-branch narrowing into
# the continuation, so the following condition still sees a
# zero-length value and fires. A non-NULL value may still be a
# zero-length vector (`f(numeric(0))` errors at `x > 0`); the unit
# twin is null_return_guard_alone_does_not_prove_non_empty.
f <- function(x = NULL) {
  if (is.null(x)) return(NULL)
  if (x > 0) NULL
}
f()
