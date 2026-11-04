# oracle: must-flag-only RY000
# R lexes `_` as the pipe placeholder and rejects it outside a `|>`
# right-hand side ("invalid use of pipe placeholder"); tree-sitter
# reads it as a symbol, which would be RY010 (#841).
x <- _
