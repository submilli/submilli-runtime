# submilli-engine

The engine behind [Submilli](https://github.com/submilli/submilli-runtime): it
compiles Submilli programs, a subset of TypeScript, to WebAssembly and runs
them in a sandbox where every effect goes through a capability check.

An embedder chooses the standard library its programs see with `Stdlib`, and
supplies the host services through traits on the store: the policy
(`SecurityCheck`), HTTP, secrets, model calls (`LlmProvider`), sub-agents
(`AgentProvider`) and skills (`SkillProvider`), among others.

For the policy, `submilli-policy` (with its `engine` feature) provides the
`SecurityCheck` the Submilli CLI and server use: permission rules per caller,
with filters on each capability's context. `submilli-build` builds packages and
reads their artifacts back from a package store.

Licensed under Apache-2.0.
