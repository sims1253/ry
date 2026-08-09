# no-diag
weighted_score <- function() {
  criteria <- list()
  criteria$a <- list(score = 1, weight = 1)
  criteria$b <- list(score = 2, weight = 0.5)
  scores <- sapply(criteria, function(x) x$score)
  weights <- sapply(criteria, function(x) x$weight)
  valid_idx <- !is.na(scores)
  sum(scores[valid_idx] * weights[valid_idx])
}
