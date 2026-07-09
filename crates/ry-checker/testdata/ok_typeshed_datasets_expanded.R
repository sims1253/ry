# no-diag
# New dataset column schemas: airquality, ChickWeight, trees, quakes,
# cars, faithful, women. Column access resolves to the right type,
# so using the results arithmetically is well-typed.
aq_ozone <- airquality$Ozone
aq_wind <- airquality$Wind
cw_weight <- ChickWeight$weight
t_girth <- trees$Girth
q_mag <- quakes$mag
c_speed <- cars$speed
f_erupt <- faithful$eruptions
w_height <- women$height
aq_scaled <- aq_wind + 1
cw_scaled <- cw_weight + 1
t_scaled <- t_girth + 1
q_scaled <- q_mag + 1
c_scaled <- c_speed + 1
f_scaled <- f_erupt + 1
w_scaled <- w_height + 1
