# Validation contract

This public operation is permitted to every caller by the package contract. The acme.write check is an optional policy probe selecting the response (allowed or handled); both paths intentionally write the same fixed record. Package fs.write authority authorizes that write independently of the probe. No caller data is written. This deliberately unusual API is a counterexample to inferring mandatory authorization from the presence of a check.
