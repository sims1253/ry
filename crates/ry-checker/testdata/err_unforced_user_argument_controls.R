# expect: RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY010
# Actuals stay eagerly checked unless the exact callee provably never
# forces their formal.
use <- function(x) x + 1L
use("oops" + 1L)

ignore <- function(x) 1L
forward_forcing <- function(x) use(x)
forward_forcing("oops" + 1L)
# Forwarding is not proven unforced, even to a callee that ignores it.
forward_ignoring <- function(x) ignore(x)
forward_ignoring("oops" + 1L)

# Conditionally forced formals keep eager checking.
conditional <- function(x, flag) if (flag) x else 1L
conditional("oops" + 1L, FALSE)

by_string <- function(x) get("x")
by_string("oops" + 1L)

reflective <- function(x) as.list(environment())
reflective("oops" + 1L)

status <- function(x) {
  s <- sys.status()
  as.list(s[[3L]][[length(s[[3L]]) - 1L]])
}
status("oops" + 1L)

# A called project helper may reach the caller's frame.
grab <- function() get("x", envir = parent.frame())
via_helper <- function(x) grab()
via_helper("oops" + 1L)

# Dispatch may run a project method that reaches the caller's frame.
print.frame_grabber <- function(o, ...) get("x", envir = parent.frame())
via_dispatch <- function(x) print(structure(1L, class = "frame_grabber"))
via_dispatch("oops" + 1L)

# Forwarded dots leave the positional binding unknown.
pick <- function(value, unused) value
spread <- function(...) pick(..., "oops" + 1L)

rebound <- function(x) 1L
rebound <- Vectorize(rebound)
rebound("oops" + 1L)

# An unforced actual never runs, so its assignment never binds.
ignore({ assigned <- 1L })
assigned
