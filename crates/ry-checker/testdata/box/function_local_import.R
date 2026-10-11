# expect: RY010
run <- function() {
  box::use(../oracle/box_module/hello[foo])
  foo()
}
run()
value <- foo
