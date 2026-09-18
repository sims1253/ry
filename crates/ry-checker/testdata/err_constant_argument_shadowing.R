# expect: RY111, RY111, RY111, RY111, RY111, RY111, RY111, RY111, RY111
# RY111 (constant-argument-shadowing): a `TRUE`/`FALSE` call argument for a
# formal an enclosing function exposes under the identical name, where the
# callee also has that formal -- the caller's value is silently ignored.
# Founding fixture: haven @ f067fb2, R/labelled.R:111 (median.labelled
# hardcoded `na.rm = TRUE` behind its own `na.rm = FALSE, ...` formals).
vec_data <- function(x) x

# The haven shape verbatim: the method's `na.rm` formal and forwarded dots
# coexist with the hardcoded inner constant.
median.labelled <- function(x, na.rm = FALSE, ...) {
  median(vec_data(x), na.rm = TRUE, ...)
}

# A required (defaultless) enclosing formal is the same dead binding.
f_required <- function(x, na.rm) {
  median(x, na.rm = FALSE)
}

# A collected user-function callee counts through its own signature.
my_sum <- function(x, na.rm = FALSE) sum(x)
f_user_callee <- function(x, na.rm = FALSE) {
  my_sum(x, na.rm = TRUE)
}

# A stubbed base callee whose signature already declared the formal.
f_base_callee <- function(x, na.rm = FALSE) {
  sum(x, na.rm = TRUE)
}

# The constant may also be FALSE against a TRUE-leaning default.
f_false <- function(x, na.rm = TRUE) {
  median(x, na.rm = FALSE)
}

# A nested closure without its own `na.rm` formal captures the enclosing
# binding; the hardcoded constant still orphans the caller's value.
f_captured <- function(x, na.rm = FALSE) {
  sapply(x, function(y) median(y, na.rm = TRUE))
}

# A nested closure with its own identically-named formal is judged by its
# own frame: its callers' value is the one ignored here.
f_inner_frame <- function(x, na.rm = FALSE) {
  g <- function(y, na.rm = TRUE) {
    median(y, na.rm = TRUE)
  }
  g(x)
}

# The native pipe lowers to the same named-argument call.
f_pipe <- function(x, na.rm = FALSE) {
  x |> median(na.rm = TRUE)
}

# Explicit qualification still resolves the base signature.
f_qualified <- function(x, na.rm = FALSE) {
  stats::median(x, na.rm = TRUE)
}
