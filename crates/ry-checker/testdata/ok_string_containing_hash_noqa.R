# expect: RY040
# A string literal value containing "# noqa" must NOT be treated as a
# suppression comment (PLAN Phase 4 item 3 / acceptance). The diagnostic
# on this line is real (1 + "a") and must still be reported, because
# the "#" is inside a string, not a comment. The lexical comment-based
# suppression filter distinguishes the two.
x <- "trailing # noqa"; y <- 1 + "a"
