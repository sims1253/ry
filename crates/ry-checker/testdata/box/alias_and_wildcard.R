# expect: RY010, RY010
# Renaming an export removes its original spelling from a wildcard.
box::use(./modules/two_values[renamed = foo, ...])
renamed
bar
foo
two_values
