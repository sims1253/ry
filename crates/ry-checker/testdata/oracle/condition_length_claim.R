# oracle: must-pass
# oracle-claim: RY002
# R rejects a known multi-element condition rather than selecting an element.
error <- tryCatch({
  if (c(TRUE, FALSE)) 1L
  NULL
}, error = identity)
stopifnot(inherits(error, "error"))
