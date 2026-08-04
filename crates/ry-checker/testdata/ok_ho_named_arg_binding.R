# no-diag
# Higher-order metadata uses formal indices, so R argument matching must happen
# before callback argument, source, length, and result-template inference.
f <- function(x) {
  # Exact names can be supplied out of order. FUN.VALUE precedes `...`, so
  # FUN.VAL is a legal partial match to that formal.
  a <- vapply(FUN.VALUE = logical(1), X = x, FUN = is.numeric)
  b <- vapply(x, is.numeric, FUN.VAL = logical(1))

  # Map and mapply invoke the callback with the actuals absorbed by `...`, not
  # with every raw argument appearing after the callback's formal position.
  # Distinct inputs and use of both callback parameters catch swapped bindings.
  d <- Map(x, x + 1, f = function(p, q) p + q)
  e <- mapply(right = x, FUN = function(p, q) p + q, left = x + 1)

  # Source and callback formals likewise resolve by name for the other base
  # higher-order signatures.
  filtered <- Filter(x = x, f = is.numeric)
  found <- Find(x = x, f = is.numeric)
  reduced <- Reduce(x = x, f = function(p, q) p + q)
  applied <- lapply(FUN = function(p) p + 1, X = x)
  simplified <- sapply(FUN = function(p) p + 1, X = x)

  # Positional calls remain supported.
  positional <- vapply(x, is.numeric, logical(1))
  called <- do.call(sum, list(x))

  # Package higher-order specs use the same formal-index matcher.
  mapped <- purrr::map_dbl(.f = function(p) p + 1, .x = x)
  mapped2 <- purrr::map2(.f = function(p, q) p + q, .y = x, .x = x)
  walked <- purrr::walk(.f = function(p) p + 1, .x = x)
  compacted <- purrr::compact(.p = is.numeric, .x = x)
  accumulated <- purrr::accumulate(.f = function(p, q) p + q, .x = x)

  list(!a, !b, d, e, filtered + 1, found + 1, reduced + 1,
       applied, simplified + 1, !positional, called, mapped + 1,
       mapped2, walked + 1, compacted + 1, accumulated)
}
f(c(1, 2))
