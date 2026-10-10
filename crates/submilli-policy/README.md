# submilli-policy

The policy engine of [Submilli](https://github.com/submilli/submilli-runtime):
per-caller permission rules and their filter language, session variables, and
auth-proxy injection of secrets into outbound HTTP.

The types are format-neutral. A Submilli blueprint file is one way to write a
policy; an embedder that keeps policy elsewhere builds a `Policy` and an
`AuthProxyPolicy` from its own format and evaluates them the same way.

With the `engine` feature, `submilli_policy::host` adapts them to
[`submilli-engine`](https://crates.io/crates/submilli-engine)'s host-service
traits: `PolicyCheck`, `PolicyAuthProxy` and `PolicySecretProvider`.

Licensed under Apache-2.0.
