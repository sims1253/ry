# oracle: must-warn RY032
f<-function(x=1L){put<-function(env)base::assign('x',c(1L,2L),envir=env);saved<-put;put<-function(env)NULL;stopifnot(x>0&&TRUE);saved(environment());if(is.null(x)||x==1L)TRUE else FALSE};
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
