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
total <- aq_wind + cw_weight + t_girth + q_mag + c_speed + f_erupt + w_height
