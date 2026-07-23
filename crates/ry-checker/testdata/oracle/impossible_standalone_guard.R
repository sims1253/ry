# oracle: must-flag
# A standalone checker rejects a known length-2 character vector; ry reports RY092.
check_string <- function(x, ..., arg = "x", call = NULL) {
  if (!is.character(x) || length(x) != 1L) {
    stop("x must be a single string")
  }
  invisible(NULL)
}

value <- c("a", "b")
check_string(value)
