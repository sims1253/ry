# oracle: must-warn RY113
# oracle-claim: RY113
# R ignores suppression comments. This comparison has no NA operand.
stopifnot(1L == 1L) # ry: ignore[RY034]
