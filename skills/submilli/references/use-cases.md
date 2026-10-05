# Choose a first use case

For a first-time developer who has not specified a workflow, offer the same
three starting points as the website, in this order. Keep the invitation short
and ask one question about which they want to try.

| Use case | Invitation | What the local example demonstrates |
| --- | --- | --- |
| API workflows | “Total this customer's charges.” | Fetch fixture records, filter and total them in code, and return a compact result under a customer-scoped rule. |
| Customer support | “Can you check my account details?” | Read a fictional customer profile under an application-bound identity, then show another customer's profile request being denied. |
| Internal operations | “Include the parent account in this investigation.” | Observe the parent-account denial, inspect the rule, and review a narrow permission change before applying it. |

Keep these names and outcomes together. Error recovery, removed restrictions,
and default-allow examples are follow-up demonstrations, not additional
first-time choices. Do not replace this menu with unrelated suggestions such
as repository maintenance or incident response for an unscoped first visit.

A developer who already named a workflow, selected a site scenario, or supplied
a concrete example has made their choice. Proceed with it without presenting
the menu again. Map the site's scenario IDs `charges`, `support` (including
`support-other`), and `policy` to the three rows respectively. A request for a
specific tutorial or the `readBalance` fixture also stays within that request.
If the developer asks you to choose, use API workflows. In an unattended run,
state that default as an assumption and keep the example read-only. Otherwise
wait for their choice before building a project.

## Implement the selected example

Follow [first run](first-run.md) for isolation, setup, execution, and cleanup.
Use fictional local data so none of these starting points needs a service
account or model key. The site playground presents synthetic recorded events;
the local trial must execute the actual runtime and report its own results.

### API workflows

Use `@acme/billing.listCharges(customerId)` guarded by
`acme.com/charges.list`, with a `customerId` field in both the capability
schema and `check(...)`. Bind `customerId=cus_northwind` in the trusted demo
application and filter the main grant by `${vars.customerId}`. Return fixture
charges with integer-cent amounts, and have the runtime program total them.
Assert the count and total computed from those records. Show the records
processed by code and the compact result returned to the caller without
claiming measured token, latency, or cost savings. Deny `cus_initech` under
the unchanged binding, and reject a missing binding.

The public [quickstart](https://submilli.ai/docs/quickstart) provides the
charge-listing example. Preserve its function, capability, and blueprint names
when following it; do not combine them with the `readBalance` fixture in
[harnesses](harnesses.md).

### Customer support

Create an offline `@acme/support` fixture with
`getCustomerProfile(customerId)` guarded by
`acme.com/customer.profile.read`. Check the requested `customerId` before
reading a profile. Bind `cus_northwind` in the demo application and permit
only that customer's record. Use explicitly fictional profile fields such as
name, contact, plan, and status. Return a short account summary, then verify
that requesting `cus_initech` with the same binding is denied before its
record is returned. Reject a missing binding too.

Explain the two limits the site calls out: the application authenticates the
person, and the service/package chooses which personal fields to return.
Blueprint permissions do not authenticate a chat participant or automatically
redact PII. Do not silently turn this choice into the balance-only example.

### Internal operations

Reuse the charge-listing fixture with an additional fictional `cus_parent`
account. Under the original customer-only rule, request that account and
observe its denial. Show the requested resource and the rule that excludes it.

Propose a second narrowly scoped `acme.com/charges.list` allow rule against
`${vars.parentCustomerId}`, with `parentCustomerId` declared required and
bound by the trusted demo application. Retain the original customer rule and
`default: deny`. Show the diff before applying it. If the developer already
authorized this exact fixture-only change, proceed; otherwise ask whether to
apply it and wait. Choosing this scenario or a runtime denial alone does not
authorize a wider grant. Do not remove the customer filter, use a wildcard, or
change the default to allow.

After approval, apply the revised blueprint and bind `cus_parent` as the
parent. Verify that the original customer and the parent are allowed, an
unrelated customer remains denied, and a missing required binding is rejected.
If approval is declined, keep the original policy and report the observed
denial. The sample parent relationship is demo data; a real application must
verify that relationship and who may change policy. `ask-human` does not
implement this workflow; see [blueprints](blueprints.md).

## Beyond the first run

For existing projects or other requested use cases, ground the proposal in the
user's systems and follow [discovery](discovery.md). Inspect package schemas
before promising a policy field. If a field is missing, add a reviewed check
or constrain the trusted application rather than relying on a prompt.

Submilli fits agent-written programs combining operations and deterministic
processing under argument-level policy. A fixed workflow may be simpler as
ordinary application code. Native dependencies, arbitrary Python, full Node.js,
and unrestricted shell execution belong in the host or a scoped external
operation, not inside the runtime. For the rationale, use
[why Submilli](https://submilli.ai/docs/why).
