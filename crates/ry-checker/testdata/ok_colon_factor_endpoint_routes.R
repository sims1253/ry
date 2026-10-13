# no-diag
# `a:g` with two factor endpoints is their interaction, a factor, so
# `+.factor` handles `x + "a"`. Each `a` below is a factor in R even where
# an inferred class could look plain: `c()` dispatches to `c.factor`, a
# fifth class name is still inherited, attribute replacement sets the
# class, and a user-defined unary `-` may return a factor. Only a literal
# endpoint proves `:` returns a plain number vector (#648).
`+.factor` <- function(e1, e2) "ok"
from_c <- function(g) {
  a <- c(factor("a"))
  x <- a:g
  x + "a"
}
from_structure <- function(g) {
  a <- structure(1L, levels = "a", class = c("c1", "c2", "c3", "c4", "factor"))
  x <- a:g
  x + "a"
}
from_class_assign <- function(g) {
  a <- 1L
  class(a) <- c("c1", "c2", "c3", "c4", "factor")
  levels(a) <- "a"
  x <- a:g
  x + "a"
}
from_old_class <- function(g) {
  a <- 1L
  attr(a, "levels") <- "a"
  oldClass(a) <- "factor"
  x <- a:g
  x + "a"
}
from_attributes <- function(g) {
  a <- 1L
  attributes(a) <- list(levels = "a", class = "factor")
  x <- g:a
  x + "a"
}
g <- factor("b")
for (f in list(from_c, from_structure, from_class_assign, from_old_class, from_attributes)) {
  stopifnot(identical(f(g), "ok"))
}

local({
  `-` <- function(x) factor("a")
  from_negation <- function(n) {
    x <- (-1):n
    x + "a"
  }
  stopifnot(identical(from_negation(factor("b")), "ok"))
})
