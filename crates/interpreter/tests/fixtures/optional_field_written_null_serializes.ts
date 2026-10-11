// Explicitly nullable optional fields retain null when written and serialized.
type Plain = { id: number; note?: string | null };
type Nullable = { id: number; note?: string | null };
interface Named { id: number; note?: string | null }
class Holder {
  id: number = 1;
  note?: string | null;
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
