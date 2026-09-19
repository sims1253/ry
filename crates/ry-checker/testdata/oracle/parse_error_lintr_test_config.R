# oracle: must-flag-only RY000
# Byte-for-byte copy of lintr's
# tests/testthat/dummy_packages/RConfigInvalid/lintr_test_config.R at
# 990e578 (the posit corpus pin), prepended with these marker lines
# only. The dangling `1 +` makes R fail with "unexpected end of input",
# so whole-file suppression (#467) must leave RY000 as the file's only
# diagnostic (#470).
# invalid R syntax
1 +
