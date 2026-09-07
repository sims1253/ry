# oracle: must-pass
# Qualified structure binds .Data first, then evaluates attributes in dots order.
x <- base::structure(.D = list(a = 1L), class = base::c("widget", "list"))
stopifnot(identical(x$a, 1L), identical(class(x), base::c("widget", "list")))
y <- base::structure(.D = "attribute", .Data = list(a = 1L), class = "widget")
stopifnot(identical(y$a, 1L), identical(attr(y, ".D"), "attribute"))
z <- base::structure(class = { marker <- "class"; "widget" },
                     .Data = { marker <- 1L; list(a = 1L) })
stopifnot(identical(marker, "class"), identical(z$a, 1L))
renamed <- base::structure(list(a = 1L), .Names = "b", class = "widget")
stopifnot(is.null(renamed$a), identical(renamed$b, 1L))
plain <- base::structure(x, class = NULL)
stopifnot(identical(class(plain), "list"), identical(plain$a, 1L))
factor_value <- base::structure(1, class = "factor")
stopifnot(identical(typeof(factor_value), "integer"))
fails <- function(expr) inherits(tryCatch(force(expr), error = function(e) e), "error")
stopifnot(fails(base::structure(x = 1L)))
stopifnot(fails(base::structure(.Data = 1L, .Data = 2L)))
stopifnot(fails(base::structure(.D = 1L, .Da = 2L)))
stopifnot(fails(base::structure(1L, 2L)))

f <- function(c) base::structure(1L, class = c("widget"))
stopifnot(identical(class(f(function(...) "actual")), "actual"))
g <- function(structure) structure(1L, class = "widget")
stopifnot(identical(g(function(...) "custom"), "custom"))

(function() {
  `::` <- function(pkg, name) function(...) "custom"
  stopifnot(identical(base::structure(missing_payload, class = missing_class), "custom"))
})()
stopifnot(identical(class(base::structure(1L, class = base::c("widget", recursive = "TRUE"))), "widget"))
