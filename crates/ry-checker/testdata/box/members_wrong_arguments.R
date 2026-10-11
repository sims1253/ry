# expect: RY090, RY090, RY090
box::use(m = ./modules/members)
m$foo(wrong = 1L)
m$`foo`(wrong = 1L)
m$"foo"(wrong = 1L)
