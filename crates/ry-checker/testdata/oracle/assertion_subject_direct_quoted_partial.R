# oracle: must-warn RY032
run <- function(env, `action` = function(...) NULL) action("x", {x <- c(1L,2L); 1L}, assign.env=env, eval.env=env)
f <- function(x=1L) { run(environment(), act=base::delayedAssign); stopifnot(x>0 && TRUE); if(is.null(x)||x==1L) TRUE else FALSE }
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
