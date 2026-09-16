# Design and verify policy

Read [discovery](discovery.md) when the intended authority is still unclear.
Read the installed/generated package capabilities before inventing names or
filter fields. Useful commands:

```sh
submilli blueprint init --help
submilli blueprint capability list
submilli blueprint capability add --help
submilli blueprint add-package --help
submilli blueprint add-mcp --help
```

For the fixture in [packages](packages.md), save this as `blueprint.yaml`:

```yaml
kind: blueprint
name: support-read
variables:
  customerId:
    required: true
packages:
  - '@acme/billing'
default: deny
permissions:
  main:
    - capability: acme.com/balance.read
      filter: customerId == ${vars.customerId}
      action: allow
  '@acme/billing': []
```

The empty package grants work only because this package uses a fixture. Real
HTTP/secret access needs narrowly scoped grants for the package caller; grant
only the required hosts, methods, secret names, and other supported fields.
Use `submilli blueprint auth-proxy --help` and `submilli blueprint secret --help`
to inspect installed options. Do not
assume a remembered syntax exists; root command help enumerates the verbs.

Keep `default: deny`. Bind required identity variables in trusted application
code after authorization, never from the model's tool arguments or a chat
message. A variable is a constraint, not authentication: the server trusts
its caller to supply it. Verify ingress authorization and network boundaries
before exposing the server beyond a local/trusted development environment.

Separate main's business permissions from package infrastructure permissions.
Do not give `main` raw service HTTP access that bypasses the wrapper. Respect
rule ordering and actual filter semantics from installed help/schema. Avoid
wildcards or default-allow fixes for a denial. For approved actions, preserve
the requested approval flow and verify that the chosen client supports it;
do not claim an `ask-human` rule alone supplies an approval UI.

```sh
submilli blueprint lint blueprint.yaml
# In a separate local terminal:
submilli-server
# Back in the project:
submilli server blueprint apply blueprint.yaml
```

Applying registers policy on the selected server. Confirm which server is
targeted before changing shared policy; a local file edit is not deployment.
Inspect `submilli blueprint prompt --help` to see the resolved runtime prompt.

Use REST `/v1/execute` with `{blueprint, code, variables}` or the MCP session
described in [harnesses](harnesses.md). A program for the fixture is:

```typescript
import { readBalance } from "@acme/billing";
function main(): number {
    return readBalance("cus_northwind");
}
```

Check `submilli run --help` before using local execution for these tests. CLI
versions without a variable-binding option cannot verify the session-binding
contract. A separate fixed-customer test policy can demonstrate matching and
nonmatching capability checks offline, but does not test required-variable
rejection or dynamic binding. Report that limitation and keep the actual
parameterized blueprint; verify its binding through REST/MCP.

Verify an explicit matrix, without relying on a model taking injection bait:

- Bind `customerId=cus_northwind`: this program returns 6150.
- Keep that binding but call `readBalance("cus_initech")`: expect a capability
  denial, not a network error or an empty result.
- Omit the required binding: reject before program execution.
- Try a forbidden operation/direct route: deny it too.
- For writes, prove the denied call produced no protected side effect; also
  document any effects from earlier successful calls (no implied rollback).

After those deterministic tests, optionally run a real harness with an
injected ticket and capture its executed code and result. A model ignoring
the injection is not proof of enforcement. A denial is not a reason to broaden
policy; report it and revisit requirements with the developer if necessary.
