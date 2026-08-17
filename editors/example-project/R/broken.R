# Purpose: a deliberate RY000 syntax-error region. The valid statements
# around it matter: they keep the file above the not-r-source heuristic
# (RY097) so the parse errors surface as RY000 instead of suppressing the
# whole file. Diagnostics leaking out of a recovered region (e.g. an
# RY010 for `dose` below) are expected: RY000's message says later
# findings in the file may be unreliable.
#
# Expected diagnostics: several RY000 spans inside brew_ratio(), plus one
# RY010 leaked from the recovered region (the `daily_shots` line after it).

oz_per_cup <- 8
beans_per_shot <- 18
grinds_per_day <- oz_per_cup * 3
shots_per_day <- grinds_per_day / beans_per_shot

brew_ratio <- function(dose {
  dose +
}

daily_shots <- shots_per_day + 1
weekly <- daily_shots * 7L
