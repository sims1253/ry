# oracle: must-flag-only RY000
# R reads a backticked name with the string escapes, so an unknown one
# fails as in a string ("'\q' is an unrecognized escape in character
# string"); tree-sitter takes the whole name as one token (#841).
`a\qb` <- 1
