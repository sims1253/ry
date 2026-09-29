# oracle: must-pass
run <- function(env, `action` = function(...) NULL) action("x", {x <- c(1L,2L); 1L}, assign.env=env, eval.env=env)
f <- function(x=1L) { run(environment(), action=function(...) NULL); stopifnot(x>0 && TRUE); if(is.null(x)||x==1L) TRUE else FALSE }
stopifnot(f())
