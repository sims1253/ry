# oracle: must-pass
# oracle-claim: RY033
# Mixed character/numeric comparison coerces the number to character: this is
# lexicographic ("10" precedes "2"), not numeric comparison.
stopifnot(isTRUE("10" < 2))
