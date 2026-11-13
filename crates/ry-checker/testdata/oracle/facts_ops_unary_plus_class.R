# oracle: must-pass
# `ry dump-facts` for unary `+` with no `Ops` method, checked against GNU R
# by the ry-cli facts oracle (#891). Like unary `-`, it coerces logical to
# integer and keeps the class only when the storage mode stays.
# oracle-facts: logical coerced kept kept_int plain
logical <- +TRUE
coerced <- +structure(TRUE, class = "foo")
kept <- +structure(1, class = "foo")
kept_int <- +structure(1L, class = "foo")
plain <- +1
