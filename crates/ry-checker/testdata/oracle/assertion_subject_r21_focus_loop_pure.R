# oracle: must-pass
f<-function(x=1L,b=TRUE){put<-function(env)NULL;repeat{if(b){put<-function(env)NULL;break};break};stopifnot(x>0&&TRUE);put(environment());if(is.null(x)||x==1L)TRUE else FALSE};
stopifnot(isTRUE(f()))
