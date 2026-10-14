# oracle: known-gap a `:` installed by eval(parse()) is not modelled, as for `+` and `[`
# R looks `:` up at call time, so a replacement installed dynamically wins
# (`attach(list(`:` = ...))` behaves the same). ry types `1.5:n` by base `:`
# and reports RY061 on `$`; it reports the same for `1:3`, `1.5 + 2`, and
# `c(1, 2)[1]` under the equivalent `+` and `[` replacements, so `:` is no
# more exposed than the other operators (#648).
from_eval <- function(n) {
  eval(parse(text = "`:` <- function(a, b) list(value = 42)"))
  x <- 1.5:n
  x$value
}
stopifnot(identical(from_eval(3), 42))
