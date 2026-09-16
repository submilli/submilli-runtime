# Where Submilli fits

Tie a proposed use case to the user's systems and a policy that can actually
be enforced. Offer a few relevant options and let the user choose the job.

| Workflow | Packages | Blueprint boundary | Useful outcome |
| --- | --- | --- | --- |
| Customer support | billing, support tickets | bound customer; read-only; approved support channel | total charges and summarize relevant tickets |
| Repository maintenance | source hosting | fixed organization/repository; branch and operation restrictions | inspect failures and propose a patch |
| Incident investigation | observability, ticketing | service/environment and time range; no production mutation | correlate evidence and draft an incident update |
| Finance operations | invoices, billing | tenant, amount limit, action type; approval for payments | reconcile invoices or prepare a refund draft |
| Internal knowledge work | document/search integrations | authorized sources and output destination | combine results into a compact report |

These are designs, not promises that all listed packages or fields already
exist. Inspect available packages/MCP bridges and their schemas. If a field
cannot be enforced by the current capability, add a reviewed operation/check
or constrain the trusted application; do not merely mention it in a prompt.

Best fit: agent-written programs need several operations plus deterministic
aggregation, and the developer needs rules over those operations' arguments.
A small fixed workflow may be simpler as ordinary application code. Native
dependencies, arbitrary Python, full Node.js, or unrestricted shell execution
do not run inside Submilli; keep those in the host or expose a carefully scoped
service operation if appropriate.

Explain the specific avoided failure (another customer's records, posting to
the wrong channel) rather than promising universal injection immunity or a
fixed token/cost saving. Move into [discovery](discovery.md) once a use case is
chosen. For the rationale, link https://submilli.ai/blog/why-submilli/.
