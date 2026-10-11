// `String(null)` is "null", as in JavaScript, whether the null comes from an
// `unknown`, a nullable union, or a field. A body that falls off its end and a
// missing map entry give `undefined`, so `String` of them is "undefined".
function nothing(): string | null {
  return null;
}

function fallsOff(): unknown {
  return;
}

function main(): void {
  const value: unknown = null;
  assert(String(value) === "null", "an unknown null");
  assert(String(nothing()) === "null", "a nullable union");
  assert(String(null) === "null", "a null literal");
  assert(String(fallsOff()) === "undefined", "an implicit unknown result");
  const holder: { a: number | null } = { a: null };
  assert(String(holder.a) === "null", "a null field");
  const unknowns: unknown[] = [null];
  const box: { v: unknown } = { v: null };
  assert(String(unknowns[0]) === "null" && String(box.v) === "null", "an unknown null read from a container");
  const nullObject: { a: number } | null = null;
  assert(String(nullObject) === "null", "a null object");
  assert(String(new Map<string, number>().get("missing")) === "undefined", "a missing map entry");
  const items: (number | null)[] = [1, null];
  assert(items.map((x) => String(x)).join(",") === "1,null", "mapped nulls");
  assert(String(items) === "1,", "an array joins null as empty");
}
