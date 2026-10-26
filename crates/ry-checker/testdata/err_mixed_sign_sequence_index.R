# expect: RY126, RY126, RY126, RY126, RY126, RY126, RY126, RY126, RY126
# Sequence subscripts whose proven endpoints straddle zero mix
# positive and negative subscripts; base `[` always errors on them.
# The `1:n` line is the literal-proven core of the dbplyr/rvest
# computed-endpoint family (#355): here `n` carries an exact value
# fact, so the crossing is proven rather than possible.
x <- 1:10
x[1:-1]
x[seq(1, -1)]
x[seq.int(2, -3)]
x[rlang::seq2(-1, 2)]
n <- -2L
x[1:n]
x[[1:-1]]
x[1:-1] <- 0L
in_fun <- function() {
  v <- 1:10
  v[1:-1]
  v[base::seq(-1, 2)]
}
