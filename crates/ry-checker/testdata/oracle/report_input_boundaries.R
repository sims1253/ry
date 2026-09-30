# oracle: must-pass
# oracle-claim: RY120
# oracle-claim: RY121
# Separate R chunks must each parse before execution. A later chunk cannot
# repair the first one's incomplete expression.
stopifnot(inherits(try(parse(text = "value <- ("), silent = TRUE), "try-error"))
stopifnot(inherits(try(parse(text = "1L)"), silent = TRUE), "try-error"))
# An eval choice evaluated at runtime can enable or disable a binding.
run_chunk <- function(choice) {
  env <- new.env(parent = baseenv())
  if (choice) eval(quote(hidden <- 1L), envir = env)
  exists("hidden", envir = env, inherits = FALSE)
}
stopifnot(run_chunk(TRUE), !run_chunk(FALSE))
