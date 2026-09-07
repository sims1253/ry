# Purpose: call-argument diagnostics on base/stats functions, plus the
# inline-suppression contract. These calls use known base signatures.
# R/resolution.R also includes a project function call that currently
# stays silent despite missing arguments (see README).
#
# Expected diagnostics:
#   RY090 + RY091 - length(xx = 1L)   (unknown arg `xx`, `x` left unbound)
#   RY091          - length()         (required `x` missing)
#   RY092          - mean("text")     (character where numeric expected)
#   RY093          - length(prices == 3)
#   RY094          - sprintf with too few value arguments
#   RY010 x4 in the suppression section: two suppressed, two live (one ASCII)

count_items <- function(items) {
  # RY090 + RY091: `xx` matches no formal of length() (did you mean `x`?),
  # and the required `x` parameter ends up unbound.
  n_bad <- length(xx = 1L)

  # RY091: length() with no arguments at all.
  n_worse <- length()

  # Correct spelling, for contrast:
  n_good <- length(items)
  n_good
}

audit_prices <- function(prices) {
  # RY092: mean()'s `x` parameter is declared numeric.
  avg_bad <- mean("not numeric")
  avg_good <- mean(prices)

  # RY093: comparison directly inside length() is a parenthesisation bug.
  n_over <- length(prices == 3)

  # RY094: two conversions in the format string, one value argument.
  label <- base::sprintf("%d of %d items", n_over)

  list(avg_bad, avg_good, label)
}

# --- Inline suppression -------------------------------------------------
# Same rule, same shape, twice. The first is suppressed by the rule-specific
# comment; the second must still squiggle. Both forms below are equivalent;
# the `# noqa` alias is shown on purpose.

suppressed_a <- misspelled_variable  # ry: ignore[RY010]
suppressed_b <- misspelled_variable  # noqa: RY010
unsuppressed <- misspelled_variable  # <- RY010 fires here

# Same rule with a non-ASCII identifier: the span covers accented
# characters. Check the full underline: the server converts byte spans
# to UTF-16 code-unit positions for LSP clients.
unsuppressed_unicode <- misspelled_variablé  # <- RY010 fires here too
