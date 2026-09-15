# Submilli — contributor and agent guidance

## Repository

This workspace contains the compiler/runtime (`interpreter`), CLI (`submilli`),
HTTP/MCP server (`submilli-server`), supporting build/blueprint/shared crates,
and the conformance suite. Maintained TypeScript packages live in `packages/`.

The runtime embeds [llm-prompt.md](llm-prompt.md) at compile time.
Package `docs/readme.md` files are build inputs and their examples are tested.
Changes must build and test from this repository without private documentation.

## Design constraints

- Keep the lexer, parser, typechecker, and code generator hand-written in Rust.
- Compile to WasmGC. `submilli-wasm` is the interpreter engine; the workspace
  dependency named `wasmtime` is an alias for it.
- Runtime and standard-library operations are Rust host functions. Preserve
  capability checks, caller attribution, and resource limits when changing them.
- Use `.ts` for new source and fixtures; `.subm` remains supported.
- Accept equivalent TypeScript syntax under the forgiveness principle. Do not
  introduce different semantics merely to accept another spelling.
- `any` and `undefined` are unsupported. Use concrete types, `unknown`, or `null`.
  Casts with `as` and non-null assertions are runtime-checked.
- Language feature changes need a fixture demonstrating their behavior.
- Runtime strings are UTF-16 code units. Operate on those units rather than
  round-tripping through Rust UTF-8 strings, which loses lone surrogates.

## Errors and capabilities

Compile errors should include source context, a caret, relevant type or function
signatures, and an actionable fix. Thread source spans through every compiler phase.

When adding or removing a gated capability, update
[the capability catalog](crates/interpreter/src/stdlib/capabilities.rs) in the same
change, including its summary, filter fields, and example filter. Blueprint
scaffolding reads this catalog.

## Code style

- Rust 2024; typed errors in library APIs. `anyhow` is appropriate at the CLI
  boundary. Avoid `unwrap()` outside tests, CLI code, and infallible paths.
- Keep functions focused, names descriptive, and control flow easy to follow.
  Prefer early returns to nesting. Keep helpers below their callers.
- Preserve ordered tables and exhaustive dispatchers: they encode layout or
  completeness constraints. Extract named operations without obscuring those rules.
- Comments explain non-obvious invariants or reasons, not change history or
  what already-readable code does.
- TypeScript classes use `private` / `private readonly`, not `#private` fields.

## Verification

Run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
SUBMILLI_FULL_TEST=1 cargo test --workspace
cargo run -p submilli -- build test
```

Use focused tests while iterating. Interpreter fixtures use assertions to verify
runtime behavior; compile-error fixtures use `// expect-error: <substring>`.
Keep snapshots when the rendered diagnostic or declaration is the contract under test.

## Documentation site

The public user book lives in `docs/`. Build it independently with:

```sh
cd docs-site
npm ci
npm run check
npm run build
```

See [docs-site/README.md](docs-site/README.md) for preview and hosting instructions.
