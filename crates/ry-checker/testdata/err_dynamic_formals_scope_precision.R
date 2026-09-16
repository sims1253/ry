# expect: RY010, RY010, RY010, RY010
# Placeholder opacity is lexically scoped, replacement-specific, and
# body-local: a formals<- inside one function body does not silence the
# same-named closure in a sibling scope, a plain literal in a body
# without any formals<-/body<- keeps its unbound-variable finding,
# environment(f) <- ... mutates the closure's enclosure without changing
# its formals or body (no opacity), and closures nested inside a
# formals-only placeholder survive the replacement, so their unbound
# names are still genuine runtime errors.
top_level_trafo <- function() {
  unbound_at_top_level
}

no_construction_here <- function(trafo = NULL) {
  if (is.null(trafo)) {
    trafo <- function() x
  }
  trafo
}

enclosure_only <- function() {
  e <- function() still_unbound
  environment(e) <- globalenv()
  e
}

formals_only_placeholder <- function() {
  f <- function() {
    g <- function() nested_survives_formals_replacement
    g
  }
  formals(f) <- alist(x = )
  f
}
