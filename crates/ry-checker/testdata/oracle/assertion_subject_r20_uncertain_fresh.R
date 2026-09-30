# oracle: must-pass
f<-function(x=1L,b=FALSE,c=TRUE){ p<-function(...)NULL; repeat { if(b){break}; if(c)p<-base::assign else p<-base::delayedAssign;break }; stopifnot(x>0&&TRUE);p("x",c(1L,2L),envir=base::new.env()); if(is.null(x)||x==1L)TRUE else FALSE };
stopifnot(isTRUE(f()))
