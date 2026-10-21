# oracle: must-pass
# R binds an assignment made in an `if` arm in the enclosing frame wherever
# the `if` sits: as a value, at the end of a braced call argument, or at the
# end of a bare block (issue #694). Only a closure's body has its own frame.
pick_value <- function(code, suffix) {
  n <- if (suffix == "") {
    hits <- TRUE
    length(code)
  } else {
    hits <- endsWith(code, suffix)
    which.max(hits)
  }
  list(n, hits)
}
stopifnot(identical(pick_value(c("ab", "cb"), "")[[2]], TRUE))
stopifnot(identical(pick_value(c("ab", "cb"), "b")[[2]], c(TRUE, TRUE)))

pick_in_handler <- function(verb) {
  withCallingHandlers({
    if (verb == "summarise") {
      sizes <- NULL
    } else {
      sizes <- 1L
    }
  }, error = function(e) stop(e))
  sizes
}
stopifnot(is.null(pick_in_handler("summarise")))
stopifnot(identical(pick_in_handler("reframe"), 1L))

pick_in_block <- function(verb) {
  {
    if (verb == "summarise") sizes <- NULL else sizes <- 1L
  }
  sizes
}
stopifnot(identical(pick_in_block("reframe"), 1L))

pick_one_arm <- function(flag) {
  n <- if (flag) { extra <- 1; 2 } else 3
  extra
}
stopifnot(identical(pick_one_arm(TRUE), 1))

# The closure's assignment never reaches the caller's frame.
pick_hidden <- function(flag) {
  n <- if (flag) { helper <- function() hidden <- 1; helper(); 1 } else 2
  exists("hidden", inherits = FALSE)
}
stopifnot(identical(pick_hidden(TRUE), FALSE))
