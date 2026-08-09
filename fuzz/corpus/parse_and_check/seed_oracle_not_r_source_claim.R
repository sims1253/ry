# oracle: must-pass
# oracle-claim: RY097
# R's parser rejects content shaped like another language rather than R source.
error <- tryCatch(
  parse(text = "function broken() { return true; }"),
  error = identity
)
stopifnot(inherits(error, "error"))
