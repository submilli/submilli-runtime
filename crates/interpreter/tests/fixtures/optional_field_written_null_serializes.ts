// A write of `null` to an optional field leaves it present, holding `null`, as
// JavaScript does. Serializing it must write `null` even when the field's
// declared type has none, as with `note?: string`: a write to `x?: T` accepts
// `T | null`.

type Plain = { id: number; note?: string };
type Nullable = { id: number; note?: string | null };
interface Named { id: number; note?: string }
class Holder {
  id: number = 1;
  note?: string;
}

function main(): void {
  const p: Plain = { id: 1, note: "x" };
  p.note = null;
  assert(JSON.stringify(p) === "{\"id\":1,\"note\":null}", "object type");

  const n: Nullable = { id: 1, note: "x" };
  n.note = null;
  assert(JSON.stringify(n) === "{\"id\":1,\"note\":null}", "nullable optional field");

  const i: Named = { id: 1, note: "x" };
  i.note = null;
  assert(JSON.stringify(i) === "{\"id\":1,\"note\":null}", "interface");

  const h = new Holder();
  h.note = "x";
  h.note = null;
  assert(JSON.stringify(h) === "{\"id\":1,\"note\":null}", "class");

  const omitted: Plain = { id: 1 };
  assert(JSON.stringify(omitted) === "{\"id\":1}", "an omitted field is still left out");
}
