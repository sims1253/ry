# no-diag
# Adjacent seq.* shapes that must stay quiet next to RY108.

# The fixed hms shape (tidyverse/hms 6fec3ad): every `to` use sits behind
# a `if (missing(to)) return(...)` early exit, so the continuation is
# proven supplied.
seq.hms <- function(from = 1, to = 2, by = NULL, ...) {
  if (!is.null(by)) {
    if (missing(to)) {
      return(seq(from, by = by, ...))
    }
    return(seq(from, to, by, ...))
  }
  if (missing(to)) {
    return(seq(from, ...))
  }
  seq(from, to, ...)
}

# seq.Date / seq.POSIXt: `to` has no default, so a defaulted endpoint can
# never be confused with a supplied one.
seq.dates <- function(from, to, by, length.out = NULL, along.with = NULL, ...) {
  m_to <- missing(to)
  if (missing(by)) {
    if (m_to) {
      seq.int(from, length.out = length.out)
    } else {
      seq.int(from, to, length.out = length.out)
    }
  } else {
    seq.int(from, to, by)
  }
}

# A guarded rebinding: the missing branch reads the default deliberately
# and the fall-through path is proven supplied.
seq.fallback <- function(from = 1, to = 9, ...) {
  if (missing(to)) {
    to <- from + 1
  }
  seq(from, to, ...)
}

# A NULL default forwards loudly (seq.default errors on a length-0 `to`),
# not silently like a value-carrying default.
seq.nulldefault <- function(from = 1, to = NULL, ...) {
  seq(from, to, ...)
}

# seq.int is a separate function, not an S3 method of the seq generic.
seq.int <- function(from = 1, to = from, ...) {
  seq.int(from, to, ...)
}

# Without `...`, colliding specifiers cannot even enter the method.
seq.fixed <- function(from = 1, to = 9) {
  seq(from, to)
}

# `from` never competes with `...`-carried specifiers in seq.default, so
# its default may be forwarded unconditionally.
seq.fromonly <- function(from = 1, to, ...) {
  seq(from, to, ...)
}

# Defused uses never force the promise.
seq.defused <- function(from = 1, to = 9, ...) {
  substitute(to)
}

# A loop variable named `to` shadows the formal inside the body, and R
# assigns it even on a zero-trip loop, so the promise is dead afterwards.
seq.loopvar <- function(from = 1, to = 9, ...) {
  for (to in 1:3) {
    print(to)
  }
  seq(from, to, ...)
}

# A statement superassignment assigns in an enclosing frame, so the local
# promise survives -- but the defaulted `by = NULL` is out of scope and
# `from` never competes with `...`-carried specifiers.
seq.super <- function(from = 1, to, by = NULL, ...) {
  from <<- from
  seq(from, to, ...)
}

# A diverging then-arm in expression position: only the supplied
# fall-through reaches the continuation.
seq.exprguard <- function(from = 1, to = 9, ...) {
  lim <- if (missing(to)) {
    return(seq(from, ...))
  }
  seq(from, to, ...)
}

# A closure defined before the guard and called after it never runs on
# the defaulted path; closure bodies and defaults are not analyzed.
seq.helper <- function(from = 1, to = 9, ...) {
  helper <- function(x = to) {
    seq(from, x, ...)
  }
  if (missing(to)) {
    return(seq(from, ...))
  }
  helper()
}

# repeat runs its body at least once, so a guard-rebind inside it
# genuinely protects the post-loop use.
seq.repeatguard <- function(from = 1, to = 9, ...) {
  repeat {
    if (missing(to)) {
      to <- from + 1
    }
    break
  }
  seq(from, to, ...)
}
