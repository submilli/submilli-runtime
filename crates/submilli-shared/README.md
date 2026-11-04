# submilli-shared

Shared support for the [Submilli](https://github.com/submilli/submilli-runtime)
server, CLI and embedders: MCP clients and discovery, authentication, encrypted
secrets, model providers and tool descriptions.

The [LLM-facing prompt](llm-prompt.md) is included in the crate and embedded at
build time. This crate is released in lockstep with `submilli-engine`,
`submilli-policy`, `submilli-build` and `submilli-blueprint`.

Licensed under Apache-2.0.
