# no-diag
# The RHS of && is evaluated only after the LHS predicate succeeds, so
# callback is known to be callable there.
call_if_function <- function(callback = NULL, value = 1L) {
  if (is.function(callback) && !callback(value)) {
    FALSE
  } else {
    TRUE
  }
}

# Likewise, a nullable scalar is known to be non-null on the RHS.
filter_population <- function(population = NULL) {
  if (!is.null(population) && population != "ALL") {
    paste0(population, "FL")
  } else {
    "ALL"
  }
}
