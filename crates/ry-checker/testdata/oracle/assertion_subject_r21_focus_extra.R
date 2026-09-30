# oracle: must-warn RY032
put <- function(env) base::assign('x', c(1L,2L), envir=env)
f <- function(x=1L) {saved<-put;stopifnot(x>0&&TRUE);base::do.call(saved,base::list(environment()));if(is.null(x)||x==1L)TRUE else FALSE};
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
