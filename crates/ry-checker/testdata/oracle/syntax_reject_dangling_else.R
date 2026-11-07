# oracle: must-flag-only RY000
# At top level a newline ends the `if`, so R rejects the `else` on the
# next line ("unexpected 'else'"); tree-sitter reads `else` as a
# symbol, which would be RY010 (#841).
if (TRUE) 1
else 2
