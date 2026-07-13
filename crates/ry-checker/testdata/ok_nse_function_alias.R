# no-diag
e <- expression
vague_dt_default <- list(
  list(c = e(seconds < 10), s = "moments ago"),
  list(c = e(minutes < 45), s = e("%d minutes ago" %s% round(minutes)))
)
