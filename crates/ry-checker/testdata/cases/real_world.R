## case: user mean with a default argument is clean
# no-diag
mean <- function(x, trim = 0, na.rm = FALSE) {
  if (!na.rm && any(is.na(x))) return(NA_real_)
  sum(x) / length(x)
}

## case: the mtcars dataset is a known value
# no-diag
df <- mtcars
head(df)

## case: subset() resolves a column name inside the data frame
# no-diag
# subset() evaluates `cyl == 4` inside `mtcars`, so `cyl` is a column.
library(magrittr)
result <- mtcars %>% subset(cyl == 4)

## case: print dispatches to a user S3 method
# no-diag
# print(x) calls print.myclass because structure() sets the class.
print.myclass <- function(x, ...) {
  cat("My class:", x$name, "\n")
  invisible(x)
}
x <- structure(list(name = "foo"), class = "myclass")
print(x)

## case: NA in a double vector keeps arithmetic and mean() well typed
# no-diag
x <- c(1, 2, NA, 4)
y <- x + 10
z <- mean(x, na.rm = TRUE)

## case: c() coercion and an if expression join are clean
# no-diag
x <- c(1L, 2L, 3L)
y <- c(x, 4.5)
z <- c(y, "end")
w <- if (length(z) > 0) z else NA

## case: a for loop accumulator is clean
# no-diag
total <- 0
for (i in 1:100) {
  if (i %% 2 == 0) {
    total <- total + i
  }
}

## case: vectorized arithmetic and which() are clean
# no-diag
x <- 1:10
y <- x * 2 + 1
ok <- y > 5
sel <- which(ok)
