# oracle: must-pass
f<-function(x=1L){put<-function(env)base::assign('x',c(1L,2L),envir=env);put<-function(env)NULL;stopifnot(x>0&&TRUE);put(environment());if(is.null(x)||x==1L)TRUE else FALSE};
stopifnot(isTRUE(f()))
