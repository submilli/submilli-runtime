// A type literal's own `toString` field is a field: it can be assigned, and
// `String` and interpolation call it. A `toString` entry added to a record
// later is found the same way, as in JavaScript.
type MaybeToString = { a: number; toString?: () => string };
type WithToString = { a: number; toString: () => string };

function main(): void {
  const p: MaybeToString = { a: 1 };
  assert(String(p) === "[object Object]", "no entry yet");
  p.toString = () => "set";
  assert(String(p) === "set" && `${p}` === "set", "the assigned field converts");

  const q: WithToString = { a: 1, toString: () => "q" };
  q.toString = () => "q2";
  assert(q.toString() === "q2" && String(q) === "q2", "a required field is reassignable");

  const r: Record<string, () => string> = {};
  assert(String(r) === "[object Object]", "an empty record");
  r["toString"] = () => "rec";
  assert(String(r) === "rec", "String finds the record's entry");
  assert(`${r}` === "rec", "and so does interpolation");
}
