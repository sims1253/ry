# no-diag
asserted_nullable_character <- function(event_var = NULL) {
  if (is.null(event_var)) {
    stop("'event_var' is required")
  }
  assert_character_scalar(event_var, "event_var")
  event_var == "CNSR"
}
