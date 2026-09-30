# oracle: must-warn RY032
f<-function(x=1L,b=FALSE,c=TRUE){ p<-function(...)NULL;for(i in 1:2){if(b)next; if(c)p<-base::assign else p<-base::delayedAssign;next };stopifnot(x>0&&TRUE);p("x",c(1L,2L),envir=environment());if(is.null(x)||x==1L)TRUE else FALSE};
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
