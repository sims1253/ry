# expect: RY001
# A bare integer literal as an `if` condition still warns: R silently
# coerces integer->logical, and `if (1L)` is a mistake, not the
# `if (length(x))` non-empty idiom (which IS suppressed -- see
# ok_if_length_idiom.R). This pins that the idiom suppression does not
# over-reach to plain numeric conditions.
if (1L) print(1)
