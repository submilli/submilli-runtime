# submilli-engine

The engine behind [Submilli](https://github.com/submilli/submilli-runtime): it
compiles Submilli programs, a subset of TypeScript, to WebAssembly and runs
them in a sandbox where every effect goes through a capability check.

An embedder chooses the standard library its programs see with `Stdlib`, and
supplies the host services through traits on the store: the policy
(`SecurityCheck`), HTTP, secrets, model calls (`LlmProvider`), sub-agents
(`AgentProvider`) and skills (`SkillProvider`), among others. The policy
engine that implements `SecurityCheck` from permission rules is
[`submilli-policy`](https://crates.io/crates/submilli-policy).

Licensed under Apache-2.0.
