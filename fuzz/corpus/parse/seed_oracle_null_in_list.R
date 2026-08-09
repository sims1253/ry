# oracle: must-pass
# NULL inside a list is preserved; R succeeds.
lst <- list(a = 1, b = NULL, c = 3)
print(length(lst))
