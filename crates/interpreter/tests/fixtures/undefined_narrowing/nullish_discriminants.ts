// A discriminant may be optional, `undefined` or `null` in some members, as
// TypeScript allows: comparing it, or switching on it with `case undefined:` and
// `case null:`, narrows the union and counts toward exhaustiveness.
type Result = { kind: "ok"; value: number } | { kind?: "err"; error: string };
type Tagged = { tag: "some"; v: string } | { tag: undefined; reason: string } | { tag: null; code: number };
function show(r: Result): string {
  if (r.kind === "ok") { return `ok ${r.value}`; }
  return `err ${r.error}`;
}
function describe(x: Tagged): string {
  switch (x.tag) {
    case "some": return x.v;
    case undefined: return x.reason;
    case null: return `${x.code}`;
  }
}
function main(): void {
  assert(show({ kind: "ok", value: 1 }) === "ok 1", "the literal arm");
  assert(show({ error: "e" }) === "err e", "the omitted optional discriminant");
  assert(describe({ tag: undefined, reason: "r" }) === "r", "an undefined discriminant");
  assert(describe({ tag: null, code: 7 }) === "7", "a null discriminant");
}
