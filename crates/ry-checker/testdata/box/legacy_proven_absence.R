# expect: RY118, RY118
# A call in an uninvoked function body cannot create a load-time name.
box::use(./modules/inert_writer[foo])
box::use(./modules/present[foo])
