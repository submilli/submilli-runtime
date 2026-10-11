// A type whose fields are all optional takes `{}` and any value that shares one
// of its fields, as in TypeScript.
type Options = { q?: string; r?: number };

function describe(o: Options): string {
  return `${o.q ?? "-"}/${o.r ?? 0}`;
}

function main(): void {
  const empty = {};
  const partial = { r: 2, extra: true };
  assert(describe(empty) === "-/0", "an empty object");
  assert(describe(partial) === "-/2", "a value sharing one field");
  assert(describe({ q: "x" }) === "x/0", "a literal naming a field");
  const both: Options | { k: number } = { k: 1 };
  assert(JSON.stringify(both) === '{"k":1}', "the other union member");
}
