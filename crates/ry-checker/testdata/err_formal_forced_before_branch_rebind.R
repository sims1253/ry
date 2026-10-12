# expect: RY091, RY091
trim <- function(df, drop) {
  if (drop) {
    df <- df[-1]
  }
  df
}
trim(drop = TRUE)
coerce <- function(x) {
  if (is.character(x)) {
    x <- as.numeric(x)
  }
  x + 1
}
coerce()
