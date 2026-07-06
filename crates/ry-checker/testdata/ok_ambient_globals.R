# no-diag
# Runtime globals and tidy-eval pronouns are ambient bindings, not local vars.
x <- .GlobalEnv$.Random.seed
y <- .Random.seed
z <- .data$col
w <- .env$value
