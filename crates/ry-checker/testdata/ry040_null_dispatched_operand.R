# expect: RY040, RY040, RY040, RY040
# A literal NULL operand reports RY040 only when the other operand is
# known to have no class attribute, so primitive arithmetic must run.
# Opaque or classed operands may dispatch to a `+` method that accepts
# NULL, such as ggplot2's trailing `+ NULL` (issue #707).
add_nothing <- function(plot) plot + NULL
prepend_nothing <- function(plot) NULL - plot
layered <- function() {
  ggplot2::ggplot() +
    ggplot2::geom_blank() +
    NULL
}
classed <- structure(1, class = "units") + NULL

# Still flagged: unclassed operands use the primitive.
scalar <- function() 1 + NULL
v <- c(1, 2, 3)
vector <- v * NULL
both <- NULL + NULL

# A field missing from a locally built list is not an intentional NULL;
# it stays flagged whatever the other operand is.
ratio <- function(p) {
  res <- list(n = 1)
  p / res$nv
}
