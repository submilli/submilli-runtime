# @acme/billing

A one-operation billing package written for the Submilli quickstart. `listCharges(customerId)`
returns the charges on one customer's account, from a fixture held in the package — there is no
network call and no configuration.

The operation declares the capability `acme.com/charges.list` with a `customerId` field, so a
blueprint can grant it under a filter that pins the customer to a value the calling application
binds per request. That is what the quickstart demonstrates.
