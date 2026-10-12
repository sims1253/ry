# no-diag
# R never evaluates an actual bound to a formal the callee cannot force.
ignore <- function(x) 1L
ignore("oops" + 1L)

second <- function(value, unused) value
second(1L, "oops" + 1L)
second(unused = "oops" + 1L, value = 2L)

after_dots <- function(..., unused) 1L
after_dots(unused = "oops" + 1L)

deferred <- function() later("oops" + 1L)
later <- function(x) NULL

outer <- function() {
  local_ignore <- function(x) 1L
  local_ignore("oops" + 1L)
}
