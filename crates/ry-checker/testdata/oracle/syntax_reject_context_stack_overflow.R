# oracle: must-flag-only RY000
# R's lexer keeps a 50-slot context stack (gram.y CONTEXTSTACK_SIZE)
# and every open bracket takes a slot, so 50 nested parentheses fail
# with "contextstack overflow"; tree-sitter accepts them (#841).
x <- ((((((((((((((((((((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))))))))))))))))))))
