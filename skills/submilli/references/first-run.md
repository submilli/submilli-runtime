# First local run with the coding assistant

Use this path when the developer asks to try Submilli locally or complete a
quickstart. The coding assistant does the setup and runs the example. The
developer should finish with editable files and observed allowed and denied
results. Installing this skill does not itself install or start the runtime.

An existing application adoption request follows [discovery](discovery.md).
For explanation-only or instructions-only requests, respond without installing,
creating files, or starting processes.
If the developer supplies a particular example or workflow, keep that scope.

## Build the first example

Read [setup](setup.md) to check the CLI and server. Work in the requested
directory. In an existing project, put a standalone trial in a separate
directory without replacing its manifests, authentication, or agent tools.

Use the offline `@acme/billing` / `support-read` fixture in
[harnesses](harnesses.md), stopping before connecting a model. The linked
references provide the snippets for its package, blueprint, test, and REST
request. Create the files from those snippets and keep their names together. The public
[quickstart](https://submilli.ai/docs/quickstart) uses `listCharges`,
`acme.com/charges.list`, and the blueprint `quickstart`; if following that
chapter, use its complete example rather than mixing it with `readBalance`
and `acme.com/balance.read` from this skill.

Explain the customer-scoped rule, then create the blueprint and its package.
Mark the fixed customer binding as demo application state. The fixture needs
no business credentials, provider key, or live model call. Do not start a
requirements interview for the fixture's already-defined read-only policy.

Compile, test, publish locally, and lint as in [harnesses](harnesses.md).
Use an isolated `SUBMILLI_HOME` for the trial's package store and server data,
with the same value in setup commands and the server process. Start a
loopback-only server with a generated local token. If the port is occupied,
choose a free port and use it consistently instead of stopping another server.
Keep the token out of generated TypeScript and the reported transcript.

Save a runnable client or commands and exercise the real runtime:

1. Bind `customerId=cus_northwind` in the application request and call
   `readBalance("cus_northwind")`. Assert the result is `6150`.
2. Keep that binding and call `readBalance("cus_initech")`. Assert a policy
   denial naming `acme.com/balance.read`, not merely a failed HTTP request.
3. Omit the required binding. Assert `invalid_request` names `customerId`.

Use [verification](verification.md) before calling the trial complete. Show
the commands and observed results, with the blueprint rule and the package's
`check(...)` call that explain them. A canned program exercises enforcement;
it does not establish that a live model followed an injected instruction.
Report blocked steps separately from executed checks.

Stop only the server this trial started. Preserve the example source and give
the commands to run it again, regenerating temporary tokens and state as needed.
Do not delete or overwrite an existing server's data to reset the demo.

## Continue from the result

If the developer wants an agent, follow [harnesses](harnesses.md) with their
chosen framework and provider. If they want to adopt Submilli in their app,
inspect that app and use [discovery](discovery.md) to establish its actual
authority. The demo identity is not production authentication.

For guided authoring, the book has coding-assistant tutorials for
[crafting a blueprint](https://submilli.ai/docs/tutorials/craft-a-blueprint)
and [building a package](https://submilli.ai/docs/tutorials/build-a-package).
Fetch the relevant chapter through https://submilli.ai/docs/llms.txt when
following it. These extend the first trial; do not require external-service
setup just to demonstrate a local permission check.
