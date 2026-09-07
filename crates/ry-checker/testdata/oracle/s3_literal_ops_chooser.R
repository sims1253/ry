# oracle: must-pass
x <- structure(1L, class = "left")
y <- structure(2L, class = "right")
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) 1L
chooseOpsMethod.left <- function(x, y, mx, my, cl, reverse) FALSE
chooseOpsMethod.right <- function(...) TRUE
right <- x + y
reversed <- y + x
chooseOpsMethod.left <- function(...) TRUE
left <- x + y
selector <- chooseOpsMethod.left
chooseOpsMethod.left <- selector
aliased <- x + y
stopifnot(identical(right, 1L), identical(reversed, 1L),
          identical(left, "left"), identical(aliased, "left"))

# Different names for the same function do not invoke the chooser.
method <- function(e1, e2) "same"
`+.left` <- method
`+.right` <- method
same <- x + y
stopifnot(identical(same, "same"))

# Default fallback remains unknown in ry: arithmetic and comparison have
# different class propagation, and neither invokes the selected S3 methods.
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) 1L
`==.left` <- function(e1, e2) TRUE
`==.right` <- function(e1, e2) FALSE
chooseOpsMethod.left <- function(...) FALSE
chooseOpsMethod.right <- function(...) FALSE
fallback <- suppressWarnings(x + y)
comparison <- suppressWarnings(x == y)
stopifnot(identical(unclass(fallback), 3L), identical(class(fallback), "left"),
          identical(comparison, FALSE), is.null(attr(comparison, "class")))

# An operand can mutate selection before dispatch begins.
change <- function() {
  assign("chooseOpsMethod.left", function(...) FALSE, envir = parent.frame())
  y
}
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) 1L
chooseOpsMethod.left <- function(...) TRUE
chooseOpsMethod.right <- function(...) TRUE
changed <- x + { change(); y }
stopifnot(identical(changed, 1L))
