# oracle: must-pass
# `$` on a list with a missing name returns NULL (R does not error
# for `$` access to a missing name -- only `[[]]` errors).
# KNOWN LIMITATION: ry currently emits RY060 (undefined column) for
# `list(a=1)$missing`, a false positive -- R's `$` returns NULL for
# missing names rather than erroring. The oracle surfaces this. The
# RY060 check should be scoped to data frames (where a missing column
# is a real bug), not plain lists.
v <- list(a = 1)$missing
print(v)
