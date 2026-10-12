# expect: RY040
# A re-export keeps its signature through six nested modules.
box::use(./modules/chain/c1[foo])
foo() + 1L
