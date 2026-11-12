// expect-error: packages cannot declare main
// A package's generic `main` is reported as a `main`, not inferred as an
// entry point: no runtime test is recorded for its bound.
interface Node { children: Map<string, Node> }
function main<T extends Node>(): void {}
