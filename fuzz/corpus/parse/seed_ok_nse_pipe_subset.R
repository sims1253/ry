# no-diag
# A magrittr pipe into `subset(cyl == 4)` desugars to
# `subset(mtcars, cyl == 4)`. The NSE handler injects `mtcars`'s column
# schema into the scope used to infer the expression argument, so `cyl`
# resolves and no RY010 is emitted.
df <- mtcars
result <- df %>% subset(cyl == 4)
