# First local run with the coding assistant

Use this path when the developer asks to try Submilli locally or complete a
quickstart. The coding assistant does the setup and runs the example. The
developer should finish with editable files and observed allowed and denied
results. Installing this skill does not itself install or start the runtime.

An existing application adoption request follows [discovery](discovery.md).
For explanation-only or instructions-only requests, respond without installing,
creating files, or starting processes.
If the developer supplies a particular example or workflow, keep that scope.

## Choose the first example

Read [use cases](use-cases.md). If the developer has not selected a workflow,
offer API workflows, Customer support, and Internal operations with their short
invitations and wait for a choice. If the request already names a workflow,
continue with it. Do not make a billing balance demo the universal default.

## Build the selected example

Read [setup](setup.md) to check the CLI and server. Work in the requested
directory. In an existing project, put a standalone trial in a separate
directory without replacing its manifests, authentication, or agent tools.

Use the selected recipe in [use cases](use-cases.md), with package and
blueprint mechanics from [packages](packages.md) and [blueprints](blueprints.md).
The [harness setup](harnesses.md) provides the server and REST request pattern;
adapt its endpoint, binding, package, capability, and expected result together.
Its `readBalance` / `support-read` fixture remains available when explicitly
requested, but does not replace the chosen profile or charge-listing example.

Explain the scoped rule, then create the blueprint and package. Mark fixed
identity bindings and account relationships as demo application state. These
fixtures need no business credentials, provider key, or live model call. Do
not interview the developer about production business policy to run a defined
fixture. For Internal operations, preserve the recipe's explicit decision
before widening the demo's permissions.

Compile, test, publish locally, and lint as in [harnesses](harnesses.md).
Use an isolated `SUBMILLI_HOME` for the trial's package store and server data,
with the same value in setup commands and the server process. Start a
loopback-only server with a generated local token. If the port is occupied,
choose a free port and use it consistently instead of stopping another server.
Keep the token out of generated TypeScript and the reported transcript.

Save a runnable client or commands and exercise the real runtime against the
selected recipe: its allowed result, a cross-customer policy denial, and a
missing-binding rejection. Assert the actual result or fields for that recipe
and the correct capability in the denial. For Internal operations, verify the
original policy first. Only after the recipe's approval condition is met, apply
and verify the revision, including an unrelated customer that must remain
denied. If approval is pending or declined, retain the original policy and
report the observed denial without claiming a revised-policy result. A failed
HTTP request alone is not proof that the permission rule ran.

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
