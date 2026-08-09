# no-diag
# NULL defaults are common optional callback slots. A positive
# is.function() guard proves the branch can call the callback.
run_callback <- function(callback = NULL) {
  if (is.function(callback)) {
    callback(1L)
  }
}
