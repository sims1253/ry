# oracle: must-pass
# R coerces if/while conditions to logical. A character scalar coerces
# only when its text is exactly one of "T", "TRUE", "true", "True",
# "F", "FALSE", "false", "False"; every other string (including "NA"
# and "") errors with "argument is not interpretable as logical".
# Escapes decode before coercion, so the hex, octal, and raw-string
# spellings below are the same literals. A computed scalar string such
# as Sys.getenv("FLAG") is valid only when its value is an accepted
# literal; an unset variable returns "" and errors at runtime.
a <- if ("TRUE") 1 else 2
b <- if ("T") 1 else 2
c <- if ("\x54RUE") 1 else 2
d <- if ("\106ALSE") 1 else 2
e <- if (r"(FALSE)") 3 else 4
f <- if ("F") 5 else 6
g <- if (as.raw(1)) 7 else 8
h <- if (as.raw(0)) 9 else 10
stopifnot(a == 1, b == 1, c == 1, d == 2, e == 4, f == 6, g == 7, h == 10)
i <- 0
while ("TRUE") {
  i <- i + 1
  break
}
stopifnot(i == 1)
accepted <- function(value) if (value) 1 else 2
stopifnot(accepted("true") == 1, accepted("False") == 2)
rejected <- tryCatch(eval(parse(text = 'if ("tRue") 1 else 2')), error = identity)
stopifnot(inherits(rejected, "error"))
na_string <- tryCatch(eval(parse(text = 'if ("NA") 1 else 2')), error = identity)
stopifnot(inherits(na_string, "error"))
# Complex scalar truthiness is the OR of both components: 0+0i is FALSE,
# 0+1i and 2+3i are TRUE. Zero-length and length-two complex vectors
# error like any other mode.
ic <- if (complex(real = 1)) 1 else 2
jc <- if (complex(real = 0)) 3 else 4
kc <- if (0+0i) 1 else 2
lc <- if (0+1i) 1 else 2
mc <- if (2+3i) 1 else 2
stopifnot(ic == 1, jc == 4, kc == 2, lc == 1, mc == 1)
zero_complex <- tryCatch(eval(parse(text = 'if (vector("complex", 0)) 1 else 2')), error = identity)
stopifnot(inherits(zero_complex, "error"))
long_complex <- tryCatch(eval(parse(text = 'if (vector("complex", 2)) 1 else 2')), error = identity)
stopifnot(inherits(long_complex, "error"))
# NA_complex_ is complex of length 1 and ERRORS at `if` ("argument is
# not interpretable as logical") — the only scalar complex shape R
# rejects. ry stays silent on it (untracked value; NA policy is #354);
# this assertion pins the runtime fact, it is not a passing shape.
na_cx <- tryCatch(eval(parse(text = 'if (NA_complex_) 1 else 2')), error = identity)
stopifnot(inherits(na_cx, "error"))

# Known multi-value logical and numeric conditions fail in both contexts.
for (code in c(
  "if (c(TRUE, FALSE)) print(1)",
  "if (c(1L, 2L)) print(1)",
  "if (c(1, 2)) print(1)",
  "while (c(TRUE, FALSE)) { break }",
  "while (c(1L, 2L)) { break }",
  "while (c(1, 2)) { break }"
)) {
  rejected_length <- tryCatch(eval(parse(text = code)), error = identity)
  stopifnot(inherits(rejected_length, "error"))
}
