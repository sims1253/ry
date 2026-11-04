# oracle: must-flag-only RY000
# Two expressions on one line without `;` ("unexpected numeric
# constant"). tree-sitter reads them as two statements, so the RY010 on
# `fjkdsfa` it would otherwise produce is an artifact (#841). Mirrors
# styler's tests/testthat/public-api/xyzaddin/addin_region-in.R.
fjkdsfa 2
