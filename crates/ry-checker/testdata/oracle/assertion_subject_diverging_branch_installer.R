# oracle: must-pass
# A branch that installs a binding and then stops contributes nothing to the
# continuation, so the earlier scalar fact survives.
f <- function(x=1L,cond=FALSE) { stopifnot(x > 0 && TRUE); if(cond) { base::assign("x",c(1L,2L),envir=environment()); stop("done") }; if(is.null(x) || x == 1L) TRUE else FALSE }; f()
