# oracle: must-warn RY032
f <- function(x=1L) { stopifnot(x > 0 && TRUE); p <- function(...) NULL; i <- 0L; while(i < 2L) { i <- i+1L; p("x", c(1L, 2L), envir=environment()); p <- base::assign }; if(is.null(x)||x==1L) TRUE else FALSE };
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
