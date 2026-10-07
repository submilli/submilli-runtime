// A Set or Map key's structural `equals` and `hash` read each field by the value
// it holds now, not by the object literal's own field type: a binding with a
// wider type can later store `null` or another type in the field.
function main(): void {
  const p: { v: number | null } = { v: 1 };
  p.v = null;
  const s = new Set<{ v: number | null }>([p]);
  assert(s.has(p), "a field reassigned to null still hashes and compares");

  const fresh = new Set<{ v: number | null }>([{ v: 5 }]);
  const r: { v: number | null } = { v: null };
  r.v = 5;
  assert(fresh.has(r), "a field built as null and reassigned compares by its value");

  const b: { ok: boolean | null } = { ok: true };
  b.ok = null;
  const m = new Map<{ ok: boolean | null }, string>([[b, "x"]]);
  assert(m.get(b) === "x", "a boolean field reassigned to null still hashes");
  assert(m.get({ ok: null }) === "x", "an equal object finds the entry");

  const t: { s: string | number } = { s: "a" };
  t.s = 1;
  const ts = new Set<{ s: string | number }>([t]);
  assert(ts.has({ s: 1 }) && !ts.has({ s: "a" }), "a field reassigned to another type");
  console.log(s.has(p), fresh.has(r), m.get(b), ts.size);
}
