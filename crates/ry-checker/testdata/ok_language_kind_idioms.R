# no-diag
# Language values that R accepts: comparison by deparse, evaluation,
# subsetting, and arithmetic after a class or method is attached.
q <- quote(a + b)
s <- quote(a)
a <- 1
b <- 2
quote(a) == "a"
q != "x"
eval(q) * 2
quote(1) + 1
length(q)
q[[1]]
fm <- y ~ x
fm[[2]]
fit_formula <- update(fm, . ~ . + z)
Ops.foo <- function(e1, e2) "dispatched"
Ops.bar <- function(e1, e2) "dispatched"
structure(quote(a + b), class = "foo") + 1
classed <- quote(a + b)
class(classed) <- "foo"
classed + 1
attr_classed <- quote(a + b)
attr(attr_classed, "class") <- "foo"
attr_classed + 1
old_classed <- quote(a + b)
oldClass(old_classed) <- "foo"
old_classed + 1
attributes_classed <- quote(a + b)
attributes(attributes_classed) <- list(class = "foo")
attributes_classed + 1
# Either operand, or any union member, may select an `Ops` method.
quote(a) + structure(1, class = "foo")
rhs <- if (runif(1) > 0.5) structure(1, class = "foo") else structure(2, class = "bar")
quote(a) + rhs
# `sapply()` cannot simplify language results; it returns a list.
calls <- sapply(1:2, function(i) quote(a))
length(calls)
local({
  quote <- function(x) 1
  quote(a) * 2
})
with_default <- function(x = quote(x)) x + 1
with_default(1)
