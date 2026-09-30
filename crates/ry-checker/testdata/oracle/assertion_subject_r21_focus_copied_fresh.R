# oracle: must-pass
f<-function(x=1L){put<-function(env)base::assign('x',c(1L,2L),envir=env);saved<-put;put<-function(env)NULL;stopifnot(x>0&&TRUE);base::do.call(saved,base::list(base::new.env()));if(is.null(x)||x==1L)TRUE else FALSE};
stopifnot(isTRUE(f()))
