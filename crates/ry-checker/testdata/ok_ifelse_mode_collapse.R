# no-diag
# Adjacent ifelse idioms that must stay quiet.
#
# A local of known length cannot be empty, and a merely maybe-NA test is
# not proof of an all-NA test: the result is honestly the branch mode.
v <- c(-1, 0, 1)
char_result <- ifelse(v > 0, "pos", "neg")
na_preserving <- ifelse(is.na(v), NA_character_, "x")
scalar_test <- ifelse(TRUE, "a", "b")
# Logical branches keep a logical result; there is no mode to lose.
logical_branches <- ifelse(v > 0, TRUE, FALSE)
# Disagreeing branch modes have no single mode to report.
mixed_branches <- ifelse(v > 0, 1L, "auto")
# A mixed test coerces the whole result to the branch mode; only an
# entirely-NA test collapses.
mixed_test <- ifelse(c(TRUE, NA), "a", "b")
# A project-local ifelse with different semantics is never the base
# function; its calls are not test-template calls.
ifelse <- function(test, yes, no) yes
own <- ifelse(logical(0), "a", "b")
# An open-world test without a typed-NA branch expresses no mode intent:
# warning here would cover every mutate pipeline, so the rule stays quiet.
open_world_plain <- function(x) ifelse(x > 0, 1, 0)
# The same for a defaulted test formal: the default is one observed
# shape, not every caller's input.
default_test <- function(test = TRUE) ifelse(test, "a", "b")
# The typed alternatives name their modes in their signatures and never
# collapse; they are not test-template calls.
typed_vctrs <- vctrs::vec_if_else(v > 0, "pos", NA_character_)
typed_dplyr <- dplyr::if_else(is.na(v), NA_real_, 1)
