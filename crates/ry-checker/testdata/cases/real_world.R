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

## case: a multi-line magrittr chain after library(magrittr)
# no-diag
library(magrittr)
result <- c(1, 2, 3) %>%
  mean() %>%
  round(2)

## case: a merge() result column takes arithmetic
# no-diag
# merge() on data frames returns a data frame, so `m[, 1]` is a column
# vector; the base stub's merge() return is opaque (#709, caret's rfe.R).
f <- function(a, b) {
  m <- merge(a, b, by = 1)
  m[, 1] - 1
}

## case: a read.csv() column selection takes arithmetic
# no-diag
# read.csv() returns a data frame, and a multi-column selection
# dispatches to Ops.data.frame (#709, parameters' methods_epi2x2.R).
g <- function(path) {
  m <- read.csv(path)
  100 * m[, c("x", "y")]
}

## case: a gc() matrix column takes arithmetic
# no-diag
# gc() returns a double matrix (#709, lobstr's mem.R).
h <- function() sum(gc()[, 1] * c(56, 8))

## case: a one-index merge() subset takes arithmetic
# no-diag
# `merge(a, a)[1]` is a one-column data frame, and Ops.data.frame
# accepts it (#709).
j <- function(a) merge(a, a)[1] + 1

## case: a one-index subset of a plain list stays a list
# expect: RY040
# `lst[1]` is still a list, and list arithmetic still errors.
k <- function() {
  lst <- list(1, 2)
  lst[1] + 1
}

## case: a data frame's character column still rejects arithmetic
# expect: RY040
k <- function() {
  df <- data.frame(x = 1, y = "a")
  df[, "y"] + 1
}

## case: a local data or load function is not the utils or base loader
# no-diag
# The Shiny `data <- reactive(...)` idiom (visNetwork): `data()` calls the local
# closure, whose result is not a character vector of dataset names (#717).
library(shiny)
server <- function(input, output, session) {
  data <- reactive({
    list(nodes = 1, edges = 2)
  })
  output$net <- renderVisNetwork({
    visNetwork(data()$nodes, data()$edges)
  })
}
f <- function() {
  data <- function() list(nodes = 1)
  data()$nodes
}
g <- function() {
  load <- function(path) list(a = 1)
  load("x")$a
}
k <- function(data) data()$nodes

## case: data still binds the dataset it loads
# expect: RY061
# Control for #717: without a local binding, `data(mtcars)` loads the
# dataset for later reads and returns the dataset names.
f <- function() {
  data(mtcars)
  mtcars$mpg
}
g <- function() data(mtcars)$x

## case: only a union with a function member shadows the data loader
# no-diag
# R's call lookup skips a function-free union, so `data(my_set)` still
# binds `my_set`; a NULL-or-closure union is the callee instead.
f <- function(flag) {
  data <- if (flag) 1 else "a"
  data(my_set)
  my_set$x
}
g <- function(flag) {
  data <- if (flag) NULL else function() list(x = 1)
  data()$x
}

## case: a data parameter with a non-function default shadows the loader
# no-diag
# A caller may pass a function over the NULL default (Shiny modules).
f <- function(data = NULL) data()$nodes
