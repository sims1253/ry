# no-diag
# A U+FEFF that is not the first bytes of the file is an ordinary
# character R's parser accepts (verified against R 4.6.1): inside
# string literals and comments it parses cleanly, so ry stays quiet.
s <- "﻿"
t <- "café"
