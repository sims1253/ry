# oracle: must-pass
local({
  f <- function(factor) { x <- factor(1L); x$a }
  stopifnot(identical(f(function(...) list(a = 1L)), 1L))
  new <- function(...) 1L
  `+.widget` <- function(e1, e2) "wrong"
  x <- new("widget")
  stopifnot(identical(x + 1L + 1L, 3L))
})
stopifnot(inherits(base::factor(1L), "factor"))
methods::setClass("ry_constructor_widget", slots = c(x = "integer"))
stopifnot(inherits(new("ry_constructor_widget"), "ry_constructor_widget"))
detach("package:methods")
stopifnot(inherits(methods::new("ry_constructor_widget"), "ry_constructor_widget"))
stopifnot(inherits(tryCatch(new("ry_constructor_widget"), error = identity), "error"))
stopifnot(inherits(tryCatch(base::new("ry_constructor_widget"), error = identity), "error"))
stopifnot(inherits(tryCatch(methods::factor(1L), error = identity), "error"))
local({
  `::` <- function(pkg, name) function(...) "custom"
  stopifnot(identical(methods::new(missing_class), "custom"))
  stopifnot(identical(base::factor(missing_value), "custom"))
})
methods::setClass("ry_new_binding", slots = c(value = "character", C = "character"))
stopifnot(inherits(methods::new(value = "wrong", Cl = "ry_new_binding"), "ry_new_binding"))
stopifnot(inherits(methods::new(C = "attribute", Class = "ry_new_binding"), "ry_new_binding"))
methods::setMethod("initialize", "ry_new_binding", function(.Object, ...) .Object)
stopifnot(inherits(methods::new("ry_new_binding", missing_value), "ry_new_binding"))
marker <- 1L
stopifnot(inherits(tryCatch(methods::new(Class = { marker <- "forced"; missing_class }), error = identity), "error"))
stopifnot(identical(marker, "forced"))
