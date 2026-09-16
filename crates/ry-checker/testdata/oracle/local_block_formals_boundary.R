# oracle: must-warn RY010
# R-side ground truth for the local() scope boundary of the formals<-
# opacity (issue #380 review): a replacement inside local({...}) (or
# inside another function's body) assigns the modified closure to that
# evaluation environment only, so the outer binding keeps the
# placeholder body verbatim and calling it errors on the name the
# alist() never installed. Literal and replacement inside one local()
# block are one scope and pair up. ry must keep the outer placeholders'
# RY010 for exactly the reason R errors here.
f <- function() undefined_name
local({ formals(f) <- alist(x = ) })
outer_untouched <- tryCatch(f(), error = identity)
stopifnot(inherits(outer_untouched, "error"))
stopifnot(identical(conditionMessage(outer_untouched), "object 'undefined_name' not found"))

mutator <- function() {
  formals(h) <- alist(x = )
}
h <- function() outer_unchanged
mutator()
sibling_scope <- tryCatch(h(), error = identity)
stopifnot(inherits(sibling_scope, "error"))
stopifnot(identical(conditionMessage(sibling_scope), "object 'outer_unchanged' not found"))

paired <- local({
  g <- function() only_via_alist
  formals(g) <- alist(only_via_alist = )
  g
})
stopifnot(identical(paired(7L), 7L))
