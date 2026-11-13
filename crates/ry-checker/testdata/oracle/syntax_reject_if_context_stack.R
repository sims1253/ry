# oracle: must-flag-only RY000
# Inside braces each pending `if` takes a slot of R's 50-slot lexer
# context stack next to its `{`, so 25 nested `if (a) {` blocks in a
# function body fail with "contextstack overflow" although they open
# only 26 braces; tree-sitter accepts them (#841).
f <- function(a) {
  if (a) {
    if (a) {
      if (a) {
        if (a) {
          if (a) {
            if (a) {
              if (a) {
                if (a) {
                  if (a) {
                    if (a) {
                      if (a) {
                        if (a) {
                          if (a) {
                            if (a) {
                              if (a) {
                                if (a) {
                                  if (a) {
                                    if (a) {
                                      if (a) {
                                        if (a) {
                                          if (a) {
                                            if (a) {
                                              if (a) {
                                                if (a) {
                                                  if (a) {
                                                    1
                                                  }
                                                }
                                              }
                                            }
                                          }
                                        }
                                      }
                                    }
                                  }
                                }
                              }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
