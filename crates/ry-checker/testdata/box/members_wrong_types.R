# expect: RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040
box::use(m = ./modules/members, r = ./modules/renamed_members)
m$text() + 1L
m$`text`() + 1L
m$"text"() + 1L
m$`te\x78t`() + 1L
m$label + 1L
m$`label` + 1L
m$"la\u0062el" + 1L
r$`my-text`() + 1L
