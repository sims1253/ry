# oracle: must-pass
frame <- data.frame(value = 3L)
stopifnot(typeof((frame + 0.5)$value) == "double",
          typeof((0.5 + frame)$value) == "double",
          typeof((frame / 2L)$value) == "double",
          typeof((2L / frame)$value) == "double",
          typeof((frame ^ 2L)$value) == "double",
          typeof((2L ^ frame)$value) == "double",
          typeof((frame %/% 2L)$value) == "integer",
          typeof((frame %% 2L)$value) == "integer")

# A column method may return a different type from its input.
`+.widget` <- function(e1, e2) "changed"
value <- structure(1L, class = "widget")
custom <- structure(list(value = value), class = "data.frame", row.names = 1L)
stopifnot(identical((custom + 1L)$value, "changed"))
