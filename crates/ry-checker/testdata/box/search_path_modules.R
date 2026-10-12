# expect: RY010
# Search-path modules bind static names opaquely.
box::use(mod/hello[answer])
value <- answer
box::use(mod/hello[renamed = answer])
value <- renamed
box::use("m" = mod/hello)
value <- m
box::use(`\x6e` = mod/hello)
value <- n
box::use(mod/hello)
value <- hello
other <- unbound
