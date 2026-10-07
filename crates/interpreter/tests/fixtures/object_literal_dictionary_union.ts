// An object literal checked against a union with a dictionary member may suit
// the dictionary even when another member names its fields: `{ a: "on" }`
// fits `{ [k: string]: string }` though `{ a: number }` has `a`. Each field
// takes as its hint every type a member gives its name, so a literal value
// keeps the type the dictionary asks for.
type Mode = "x" | "y";

function show<A>(o: { [k: string]: A } | { a: number }): string {
  return JSON.stringify(o);
}

function modes(o: { [k: string]: Mode } | { a: number }): string {
  return JSON.stringify(o);
}

function lists(o: { [k: string]: number[] } | { a: number }): string {
  return JSON.stringify(o);
}

function main(): void {
  assert(show({ a: "on" }) === '{"a":"on"}', "fits the dictionary with a string");
  assert(show({ a: 1 }) === '{"a":1}', "fits the object member");
  assert(modes({ k: "x" }) === '{"k":"x"}', "a literal keeps the dictionary's literal type");
  assert(lists({ k: [] }) === '{"k":[]}', "an empty array takes the dictionary's element type");
  const flags: { [k: string]: boolean } | { a: number } = { a: true };
  assert(JSON.stringify(flags) === '{"a":true}', "an annotated binding");
}
