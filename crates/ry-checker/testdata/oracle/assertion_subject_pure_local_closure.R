# oracle: must-warn RY032
# Conservative boundary: R succeeds because the closure is pure, but any
# local closure call drops the scalar fact, as on main.
f<-function(x=1L){
put<-function(env)NULL
stopifnot(x>0&&TRUE)
put(environment())
if(is.null(x)||x==1L)TRUE else FALSE
};
stopifnot(isTRUE(f()))
