# oracle: must-pass
f<-function(x=1L,b=TRUE){if(b)put<-function(env)NULL else put<-function(env)NULL;stopifnot(x>0&&TRUE);put(environment());if(is.null(x)||x==1L)TRUE else FALSE};
stopifnot(isTRUE(f()))
