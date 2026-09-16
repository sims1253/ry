# no-diag
# Closures completed by the replacement-function machinery after a
# placeholder literal reference names only the construction installs
# (distr6 genExp / makeChecks, issue #380). The placeholder's internals
# stay quiet: formals(x) <- alist(...) binds the names at runtime, and
# body(x) <- substitute(...) replaces the walked body entirely.
gen_exp <- function(trafo = NULL) {
  if (is.null(trafo)) {
    trafo <- function() {
      return(x)
    }
    formals(trafo) <- alist(x = ) # nolint
  }
  trafo
}

make_check <- function(cond, errormsg, args = alist(object = , errormsg = )) {
  value <- function() {}
  formals(value) <- args
  body(value) <- substitute(assertThat(object, arg1, errormsg), list(arg1 = cond))
  value
}

body_rebuilt <- function() {
  f <- function() {
    from_body_replacement
  }
  body(f) <- substitute(from_body_replacement + 1)
  f
}

body_rebuilt_with_nested <- function() {
  f <- function() {
    g <- function() from_body_replacement
    g
  }
  body(f) <- substitute(from_body_replacement + 1)
  f
}
