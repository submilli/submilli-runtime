# Demo data: @submilli/chargeafter-demo

A self-contained fictional ChargeAfter fixture package for agent and policy demonstrations. It is not connected to ChargeAfter, does not make HTTP calls, and does not read secrets.

Install it in a blueprint as `@submilli/chargeafter-demo`. The package has no infrastructure grants. Grant the `chargeafter.com/...` capabilities to `main` exactly as you would for `@submilli/chargeafter`; those names and check fields are intentionally identical.

This package is a demo fixture and is intentionally not listed among Submilli's curated packages. Switch to `@submilli/chargeafter` only when the application is configured with valid ChargeAfter credentials.

See [the package documentation](docs/readme.md) for the operations, fictional coverage, and capability fields.
