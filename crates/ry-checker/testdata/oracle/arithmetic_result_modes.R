# oracle: must-pass
# Primitive / and ^ promote integer/logical operands; %% and %/% do not.
stopifnot(typeof(1L / 2L) == "double", typeof(2L ^ 3L) == "double",
          typeof(2L ** 3L) == "double", typeof(TRUE / FALSE) == "double",
          typeof(3L %/% 2L) == "integer", typeof(3L %% 2L) == "integer",
          identical(integer() / 1L, numeric()),
          identical(integer() ^ integer(), numeric()),
          typeof(1i / 2L) == "complex", typeof(1i ^ 2L) == "complex")

# Empty complex arithmetic returns before R rejects these operators.
stopifnot(identical(1i %% integer(), complex()),
          identical(1i %/% integer(), complex()))
mod_error <- tryCatch({ 1i %% 2L; FALSE }, error = function(e) TRUE)
div_error <- tryCatch({ 1L %/% 2i; FALSE }, error = function(e) TRUE)
stopifnot(mod_error, div_error)

# Primitive coercion does not apply to a selected method's return value.
`/.widget` <- function(e1, e2) 1L
`^.widget` <- function(e1, e2) "power"
x <- structure(2L, class = "widget")
stopifnot(identical(x / 2L, 1L), identical(x ^ 2L, "power"))
