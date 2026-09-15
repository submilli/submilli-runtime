// A closure parameter resolves through its own path, not `resolve_param`, so
// it needs the screen separately from a declared function's parameter.
// expect-error: `void` cannot be a parameter type — it has no values
function main(): void {
  const f = (x: void): number => 1;
}
