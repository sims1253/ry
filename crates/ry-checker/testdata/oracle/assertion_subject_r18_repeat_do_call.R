# oracle: must-warn RY032
f <- function(x=1L) { stopifnot(x > 0 && TRUE); p <- function(...) NULL; i <- 0L; repeat { i <- i+1L; base::do.call(p, base::list("x", c(1L, 2L), envir=environment())); p <- base::assign; if (i == 2L) break }; if(is.null(x)||x==1L) TRUE else FALSE };
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
