# expect: RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY080
# (the sixth RY010 is the `g` receiver itself: formals(g) <- evaluates
# g before any binding exists, exactly where R errors 'object "g" not
# found')
# Placeholder opacity is lexically scoped, source-ordered, and
# replacement-specific: a formals<- inside one function body does not
# silence the same-named closure in a sibling scope; a plain literal in
# a body without any formals<-/body<- keeps its unbound-variable
# finding; environment(f) <- ... mutates the closure's enclosure without
# changing its formals or body (no opacity); closures nested inside a
# formals-only placeholder survive the replacement, so their unbound
# names are still genuine runtime errors; a rebind ends the
# literal/replacement association, so the rebound literal and a
# replacement that precedes any literal keep their findings; a
# replacement inside local({...}) cannot reach an outer literal (the
# block runs in a fresh environment); and a typed-map call inside a
# formals-only body is genuine (RY080 anchors at the map call and only
# body<- discards it).
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

rebind_ends_association <- function() {
  f <- function() x
  formals(f) <- alist(x = )
  f <- function() rebind_keeps_finding
  f
}

replacement_before_literal <- function() {
  formals(g) <- alist(h = )
  g <- function() later_literal_keeps_finding
  g
}

local_boundary <- function() {
  f <- function() outer_unchanged_by_local
  local({ formals(f) <- alist(x = ) })
  f
}

library(purrr)
formals_only_typed_map <- function() {
  f <- function() {
    map_dbl(1:3, function(z) "nope")
  }
  formals(f) <- alist(x = )
  f()
}
