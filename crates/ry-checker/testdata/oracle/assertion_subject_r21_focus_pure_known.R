# oracle: must-pass
f <- function(x=1L){put<-function(env)base::identity(NULL);stopifnot(x>0&&TRUE);put(environment());if(is.null(x)||x==1L)TRUE else FALSE};
stopifnot(isTRUE(f()))
