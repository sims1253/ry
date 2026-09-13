# oracle: must-flag
# Verified in R with testthat installed — test_that(), describe(), and
# it() all route through test_code(), which evaluates the captured
# body via eval(code, new.env(parent = caller_env)). Bindings created in
# one block never reach a sibling block or the file top level, while the
# parent chain stays readable and `<<-` climbs past the fresh
# environment. Ordinary braced arguments keep R's caller evaluation
# (#350): identity({x <- 1}) writes through. The script errors at the
# same-block shadowing call, so the file as a whole is must-flag; ry's
# pinned diagnostics live in the `testthat_block_environment_oracle`
# unit test.
library(testthat)

prob <- function(x) sum(x)

test_that("block A binds a loop variable named prob", {
  for (prob in list(1, 2)) {
    NULL
  }
})

# oracle: `prob` here resolves through the parent chain to the file-level
# function, never to block A's loop binding (R: the sibling test errors
# with object-not-found were it to rely on that binding).
test_that("block B calls the file-level generic", {
  prob(1:2)
})

# oracle: must-error RY070 — a same-block binding is the call-time value
# (R errors "could not find function \"shadowed\"").
test_that("same-block shadowing is a real error", {
  shadowed <- 1
  shadowed(2)
})

# oracle: file-level bindings stay readable inside every block (the new
# environment's parent is the file environment).
file_level <- 1L
test_that("reads through the parent chain", {
  file_level + "s"
})

# oracle: `<<-` climbs past the per-test environment into the caller.
# The rebinding exists afterwards, but its precise type is not known.
test_that("superassignment escapes the block", {
  esc <<- 1L
})
esc
