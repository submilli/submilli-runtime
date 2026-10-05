# Roadmap

![Submilli: what’s next](.github/assets/roadmap.svg)

We want agents to carry out longer tasks, recover when things go wrong, and bring people in when a decision needs them. These are four areas we plan to build in Submilli.

| [Durability](#durability) | [Public package registry](#public-package-registry) | [Ask human](#ask-human) | [Managed cloud](#managed-cloud) |
| :--- | :--- | :--- | :--- |
| Resume interrupted work | Discover and share Packages | Pause for a person | Run without managing servers |

> [!NOTE]
> **Planned direction, no fixed dates.** Scope and order may change. The examples below describe intended behavior. See the [documentation](https://submilli.ai/docs/) and [release notes](https://github.com/submilli/submilli-runtime/releases) for what is available today.

---

## Durability

**Resume a program after an interruption without starting its work again.**

We want execution state and completed results to survive a restart, so a long task can continue from where it stopped.

**Example:** An agent's code fails with an error partway through a task. The agent fixes the code and continues execution from the point of failure, preserving completed work instead of starting over.

External effects need explicit recovery rules: when a service's response is lost, the runtime must distinguish a completed action from one whose outcome is unknown.

---

## Public package registry

**Discover, publish, and install Submilli Packages in one place.**

We plan a public registry for [Packages](https://submilli.ai/docs/packages), similar to npm. Package authors could publish versioned releases, and developers could find integrations and reuse them in their agents.

**Example:** A developer publishes a Package for a billing API. Another team finds it in the registry, checks its documentation and capabilities, and installs a specific version in their project. Their [Blueprint](https://submilli.ai/docs/blueprints) controls which operations the agent can use.

---

## Ask human

**Pause for approval or missing information, then continue with the response.**

We want this to work within the runtime's permission system, with a record of what was requested and what the person decided.

**Example:** A [Blueprint](https://submilli.ai/docs/blueprints) could allow small refunds automatically and require approval for larger ones. Approval should apply to the specific requested action, and a refusal should leave that action unexecuted. The existing `ask-human` policy action denies execution today; the approval and resume flow is planned.

---

## Managed cloud

**Run Submilli without operating the servers yourself.**

We plan a hosted service alongside the open source runtime, with a free tier and managed operation, scaling, and support.

The goal is to take the Blueprints and Packages you use locally into a hosted environment, while keeping permissions and resource limits under your control.

---

## Help shape the work

Tell us which of these would help you ship, and what you need it to do. Share a use case in a [GitHub issue](https://github.com/submilli/submilli-runtime/issues) or join us on [Discord](https://discord.gg/VphpukeGGj).
