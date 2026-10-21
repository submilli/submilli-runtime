// expect-error: expected `never`, got `number`
// An empty array argument infers `never`, which a later argument replaces,
// but a `never[]` receiver's element type is no candidate: nothing can be
// pushed into it, as tsc reports.
function main(): void {
  const empty = Array.from([]);
  empty.push(2);
}
