---
title: "Server settings"
description: "Every server setting, its three forms and their precedence, directories, tokens and roles, the outbound network block, telemetry, health, logs, and shutdown."
slug: reference/server-settings
---

This chapter is being written.


## Logs

Each REST or MCP program dispatched for compilation and execution emits one
`info` event on the `submilli_server::execute` target:

```text
INFO submilli_server::execute: execution finished blueprint="support" session="…" fuel=38000026 memory_peak=65536 wall_ms=2843 outcome="ok"
```

The fields are `blueprint`, `session`, `fuel` consumed, `memory_peak` in bytes,
`wall_ms`, and `outcome`. Outcomes are `ok`, `fuel_exhausted`, `timeout`,
`memory_exhausted`, or `error`. Compile failures report zero fuel and memory
because no execution store was created. Replayed idempotent responses do not
execute again and do not produce another execution event.

Memory peak includes the engine's memory reservations and charged host
allocations; it is not process RSS. Wall time covers the runner's compilation,
execution, and cleanup, excluding request validation and import discovery.
These figures are logs only and are not added to the execute response. To see
local usage on standard error, use `submilli run --report script.ts`.
