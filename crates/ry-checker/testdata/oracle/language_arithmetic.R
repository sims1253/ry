# oracle: must-flag
# oracle-claim: RY040
# `quote()` of a call builds a classless language object; primitive
# arithmetic rejects it with "non-numeric argument to binary operator" (#648).
q <- quote(a + b)
q * 2
