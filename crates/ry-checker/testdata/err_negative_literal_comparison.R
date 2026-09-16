# expect: RY105, RY105, RY105, RY107, RY107, RY107, RY107
# Negative-literal bounds in the constant-comparison families (#477).
# `any()`/`all()` coerce to 0/1 under numeric comparison, so a negative
# bound makes the guard constant no matter what the test vector holds:
# `> -1` is a dead always-TRUE guard (even for an all-FALSE vector --
# FALSE coerces to 0, still greater than -1), and `< -1`/`== -1` are dead
# always-FALSE branches. `length()` of a length-1-by-construction value is
# exactly 1 and never negative, so the same bounds kill the emptiness
# guard. The quiet neighbors at the bottom pin the boundary: `> 0`
# preserves the scalar logical's value (diffobj's idiom) and `== 1` is a
# deliberate scalar assertion.
dead_true <- function(x) if (any(x) > -1) 1 else 2
dead_false <- function(x) if (any(x) < -1) 1 else 2
dead_equal <- function(x) if (any(x) == -1) 1 else 2
dead_mirror <- function(x) if (-1 < any(x)) 1 else 2
len_true <- function(v) if (length(sum(1L)) > -1) 1 else 2
len_false <- function(v) if (length(sum(1L)) < -1) 1 else 2
len_equal <- function(v) if (length(sum(1L)) == -1) 1 else 2
keep_value <- function(x) if (any(x) > 0) 1 else 2
keep_assert <- function(v) if (length(sum(1L)) == 1) 1 else 2
