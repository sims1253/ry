# expect: RY118, RY118, RY118, RY118, RY118, RY118
box::use(m = ./modules/members)
m$missing
m$`miss\x69ng`
m$"missing"
m$missing()
m$`miss\x69ng`()
m$"missing"()
