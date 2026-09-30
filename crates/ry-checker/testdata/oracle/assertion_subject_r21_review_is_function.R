# oracle: must-warn RY032
f<-function(x=1L,b=TRUE){p<-function(...)NULL;repeat{if(b){p<-base::assign;break};break};stopifnot(is.function(p));stopifnot(x>0&&TRUE);p("x",c(1L,2L),envir=environment());if(is.null(x)||x==1L)TRUE else FALSE};
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
