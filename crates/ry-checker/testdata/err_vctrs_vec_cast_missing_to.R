# expect: RY091
# vctrs::vec_cast() requires both `x` and `to`.
vctrs::vec_cast(1)
vctrs::vec_cast(1, double())
vctrs::vec_cast(to = double(), x = 1L)
