# expect: RY070
# Null-narrowing is branch-local: after the `if`, `x` reverts to its
# pre-`if` type (NULL), so the post-`if` call must still fire RY070.
# This pins the fix for the merge_branch_bindings leak.
f <- function() {
  x <- NULL
  if (is.null(x)) {
    1
  } else {
    x(2)
  }
  x(3)
}
