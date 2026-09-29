# oracle: must-pass
run <- function(env, action, ...) action()
install <- function() run(
  env = parent.frame(),
  action = function() NULL,
  unused = base::delayedAssign
)
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
