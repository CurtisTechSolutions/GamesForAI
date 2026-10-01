# Glicko-2 ratings

The service domain exposes pure `Rating::update_period` and `rate_pair` functions.
New players start at 1500 with RD 350 and volatility 0.06. The default application
choice for the volatility constraint is tau 0.5; callers must pass it explicitly.
All results in a period use opponents' pre-period ratings, and pair updates use
both original ratings to avoid order bias. Empty periods increase RD only.

The implementation follows [Mark Glickman's revised Glicko-2 procedure](https://www.glicko.net/glicko/glicko2.pdf), with bounded numerical iteration.
Tests reproduce the published example and cover draws, inactivity, fixed
calibration anchors, extreme rating gaps and invalid numeric inputs.
Approximate 95% intervals are the rating plus/minus two RD.

A fixed anchor keeps its rating, RD and volatility while counting games. Only
measured calibration should establish such anchors; current built-in opponent
levels remain uncalibrated. Game identity and assistance configuration must
scope rating pools in the service. Truncated or otherwise unrated results must
not be converted to draws. Persistent match-completion integration follows
this mathematical unit.
