# no-diag
# Closures completed by the replacement-function machinery after a
# placeholder literal reference names only the construction installs
# (distr6 genExp / makeChecks, issue #380). The placeholder's internals
# stay quiet: formals(x) <- alist(...) binds the body-proper names at
# runtime, and body(x) <- substitute(...) discards the walked body
# wholesale -- nested closures and typed-map calls included.
gen_exp <- function(trafo = NULL) {
  if (is.null(trafo)) {
    trafo <- function() {
      return(x)
    }
    formals(trafo) <- alist(x = )
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

body_rebuilt_with_typed_map <- function() {
  f <- function() {
    map_dbl(1:3, function(z) "nope")
  }
  body(f) <- substitute(as.numeric(x))
  f
}

local_scoped_pair <- function() {
  out <- local({
    f <- function() x
    formals(f) <- alist(x = )
    f
  })
  out
}
