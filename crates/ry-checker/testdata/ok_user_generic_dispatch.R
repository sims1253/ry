# no-diag
# Project `UseMethod` generics stay silent unless dispatch is proven to
# miss: a default method, an aliased method, an unknown, guarded, or S4
# class (S4 superclasses also dispatch), a local shadowing binding, or a
# generic with no project methods. Dispatch follows the actual bound to the
# first formal (including partial matching), and a method bound in the
# calling environment wins over the global one.
area <- function(shape, ...) {
  UseMethod("area")
}
area.default <- function(shape, ...) 0
area.circle <- function(s, ...) 3.14
area.ring <- area.circle
circle <- structure(list(), class = "circle")
blob <- structure(list(), class = "blob")
ring <- structure(list(), class = "ring")
area(circle) + 1
area(blob) + 1
area(ring) + 1

describe <- function(x) UseMethod("describe")
describe.circle <- function(x) "circle"
describe_any <- function(value) describe(value)
describe_guarded <- function(value) if (inherits(value, "blob")) describe(value)
setClass("Base", representation(x = "numeric"))
setClass("Derived", contains = "Base")
describe.Base <- function(x) "base"
describe(new("Derived", x = 1))
describe_local <- function() {
  describe <- function(x) 1
  describe(blob) + 1
}

extend <- function(x, ...) UseMethod("extend")
extend(blob)

choose_one <- function(object, other) UseMethod("choose_one")
choose_one.foo <- function(object, other) "text"
choose_one.bar <- function(object, other) 1
choose_one(structure(1, class = "foo"), obj = structure(1, class = "bar")) + 1

tag <- function(x) UseMethod("tag")
tag.foo <- function(x) "text"
tag_locally <- function() {
  tag.foo <- function(x) 1
  tag(structure(1, class = "foo")) + 1
}
tag_locally()
