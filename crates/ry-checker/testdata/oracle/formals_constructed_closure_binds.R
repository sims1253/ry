# oracle: must-pass
# R-side ground truth for the formals<- opacity (issue #380): the names
# a placeholder body references are bound by the alist() the
# construction site installs, so the runtime closure executes cleanly
# where the static placeholder analysis would call them unbound.
gen_exp <- function(trafo = NULL) {
  if (is.null(trafo)) {
    trafo <- function() {
      return(x)
    }
    formals(trafo) <- alist(x = )
    stopifnot(identical(names(formals(trafo)), "x"))
    stopifnot(identical(trafo(42), 42))
  }
  invisible(NULL)
}
gen_exp()

value <- function() {}
formals(value) <- alist(object = , errormsg = )
body(value) <- substitute(assertThat(object, errormsg), list())
stopifnot(identical(names(formals(value)), c("object", "errormsg")))
assertThat <- function(object, errormsg) invisible(object)
print(value(TRUE, "bad"))
