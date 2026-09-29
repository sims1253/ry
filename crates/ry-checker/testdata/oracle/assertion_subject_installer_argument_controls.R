# oracle: must-pass
local_alias <- function() {
  put <- base::delayedAssign
  put("y", 1L, assign.env = base::new.env())
}
reordered_local <- function() {
  base::delayedAssign("y", 1L, base::new.env(), eval.env = base::new.env())
}
unused_default <- function(env, act = function() {
  base::makeActiveBinding("x", function() c(1L, 2L), env)
}) base::invisible(NULL)
pure_default <- function(act = function() base::invisible(NULL)) act()
unused_chain <- function(env,
  second = function() base::makeActiveBinding("x", function() c(1L, 2L), env),
  first = function() base::invisible(NULL)) first()
pure_chain <- function(
  second = function() base::invisible(NULL),
  first = function() second()) first()
f <- function(x = 1L) {
  local_alias()
  reordered_local()
  unused_default(environment())
  pure_default()
  unused_chain(environment())
  pure_chain()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
