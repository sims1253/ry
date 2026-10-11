# expect: RY001 RY001 RY001 RY001 RY001 RY001 RY002 RY002
# Known zero-length conditions fail in `if` and `while`; known multi-element
# conditions fail as RY002 in `if` and RY001 in `while`. A binding carries the
# same length. One-element `c()` conditions stay quiet.
if (logical(0)) TRUE else FALSE
while (logical(0)) break
if (c(TRUE, FALSE)) TRUE else FALSE
while (c(TRUE, FALSE, TRUE)) break

zero <- logical(0L)
if (zero) TRUE else FALSE
while (zero) break
many <- c(TRUE, FALSE)
if (many) TRUE else FALSE
while (many) break

one <- c(TRUE)
if (one) TRUE else FALSE
while (c(FALSE)) break
if (c(FALSE)) TRUE else FALSE
