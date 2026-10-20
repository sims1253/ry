# oracle: must-flag
# Call position skips non-function bindings, so a local environment named
# `registry` leaves no function to call ("could not find function") (#648).
registry <- new.env()
registry()
