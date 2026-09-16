# Explain Submilli

Submilli runs small TypeScript programs written by an agent under policy the
developer controls. The model can combine calls, loops, and calculations in one
program and receive a compact result. Its existing harness still runs the model.

Use a concrete example: a support agent may list the signed-in customer's
charges. A package exposes `listCharges(customerId)` and checks a capability;
a blueprint allows that capability only when `customerId` matches a variable
bound by the trusted application. An injected ticket asking for another
customer's charges cannot change that binding. The denied call does not run;
earlier effects are not rolled back and an uncaught denial stops the program.

Vocabulary, in the order a new user needs it:

- Package: reviewed, typed operations over a service, with documentation and
  checks before protected work. A Submilli package is not an npm package.
- Capability: the named operation and policy-visible fields declared by the
  package; `@capability` supplies schema, `check(...)` enforces it at runtime.
- Blueprint: the operator's YAML selection of packages, rules, credentials,
  and limits. Start with `default: deny`; permissions are per caller.
- Variable: a request/session fact supplied outside generated code. An ID can
  be visible in a prompt; the security property is that code cannot rebind it,
  not that the ID must be secret.
- Execution server: `submilli-server`, reached over MCP or REST. It does not
  choose a model or run the model loop. It is distinct from an agent harness.

Sandbox isolation and argument policy solve different problems. Policy can
constrain customer IDs, channels, repositories, and operation types, rather
than only hosts or filesystem access. Enforcement depends on correct checks,
grants, binding, and trusted ingress. Do not promise that arbitrary programs
or a separate harness's other tools are covered. A direct API tool or shell
with the same credentials can bypass this boundary outside Submilli.

The runtime compiles a strict TypeScript subset to WasmGC and runs in-process.
Avoid unsupported performance or cost guarantees. Direct HTTP is possible
when explicitly granted; package-only access is a policy design, not an
unconditional claim about every blueprint. Generated code cannot read secret
values through `submilli:secrets`; package code or an auth proxy handles them.

Public links: [introduction](https://submilli.ai/docs/introduction),
[quickstart](https://submilli.ai/docs/quickstart),
[how it works](https://submilli.ai/docs/how-submilli-works),
[why Submilli](https://submilli.ai/blog/why-submilli/).
Do not invent SDK packages, hosted services, or website routes. Some book
chapters may still be placeholders; installed help and declarations are useful
offline sources of truth.
