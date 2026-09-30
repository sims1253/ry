# oracle: must-pass
f<-function(x=1L,b=TRUE){p<-function(...)NULL;repeat{if(b){p<-function(...)NULL;break};break};stopifnot(is.function(p));stopifnot(x>0&&TRUE);p("x",c(1L,2L),envir=environment());if(is.null(x)||x==1L)TRUE else FALSE};
stopifnot(isTRUE(f()))
