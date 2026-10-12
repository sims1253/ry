# no-diag
# Quoted and escaped member names identify the same exports.
box::use(m = ./modules/members, r = ./modules/renamed_members)
value <- m$foo
value <- m$`foo`
value <- m$"foo"
value <- m$`\x66oo`
value <- m$"\u0066oo"
m$foo() + 1L
m$`foo`() + 1L
m$"foo"() + 1L
m$`\x66oo`() + 1L
m$"\u0066oo"() + 1L
value <- r$`my-fn`
r$`my-fn`() + 1L
r$"my-\u0066n"() + 1L
