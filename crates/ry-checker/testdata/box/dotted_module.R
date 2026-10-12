# no-diag
# box appends the suffix to the full dotted module name.
box::use(../oracle/box_module/foo.bar[answer])
answer() + 1L
box::use(./modules/upper/foo.bar[foo])
foo() + 1L
