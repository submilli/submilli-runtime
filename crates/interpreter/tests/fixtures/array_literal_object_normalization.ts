// Object literals with different fields in one array literal hold tsc's
// normalized union: each member gains the fields only others declare, as
// optional `null`, so every field reads from any element. A field holding
// objects is normalized the same way one level down.
function show(value: string | number | boolean | null): string {
  return value === null ? "null" : String(value);
}

function main(): void {
  const rows = [{ a: 0 }, { a: 1, b: "x" }, { a: 2, b: "y", c: true }];
  let out = "";
  for (const row of rows) {
    out += show(row.a) + ":" + show(row.b ?? null) + ":" + show(row.c ?? null) + " ";
  }
  assert(out === "0:null:null 1:x:null 2:y:true ", "fields from later elements");

  const mixed = [{ a: 1, b: 2 }, { a: "abc" }, {}];
  assert(mixed.map((m) => show(m.a ?? null) + show(m.b ?? null)).join(",") === "12,abcnull,nullnull", "differing field types");

  const same = [{ k: 1 }, { k: "s" }];
  assert(same.map((x) => (typeof x.k === "string" ? "s" : "n")).join("") === "ns", "same fields, other types");

  const methods = [{ f: (x: number) => x + 1 }, { f: (x: number) => x * 2, g: 1 }];
  assert(methods.map((o) => o.f(5)).join(",") === "6,10", "function fields");

  const flag = rows.length > 5;
  const nested = [{ kind: "a", pos: { x: 0, y: 0 } }, { kind: "b", pos: flag ? { a: "x" } : { b: 0 } }];
  const second = nested[1];
  assert(show(second.pos.x ?? null) === "null" && show(second.pos.b ?? null) === "0", "nested fields");

  const opts: { foo?: string; bar?: string } = { foo: "f" };
  const chosen = flag ? {} : opts;
  assert(show(chosen.foo ?? null) === "f", "{} joins an all-optional object as that object");

  const tagged = [{ name: "a", tags: ["x"] }, { name: "b", tags: [] }];
  assert(tagged.map((t) => t.name + String(t.tags.length)).join(",") === "a1,b0", "a field typed by the elements before it");

  const json = JSON.stringify([{ a: 0 }, { a: 1, b: "x" }]);
  assert(json === '[{"a":0},{"a":1,"b":"x"}]', "a missing field stays missing");

  const n = rows.length;
  const computed = [{ id: 1 }, { id: 2, v: n + 1 }, { id: 3, v: rows[0].a, w: show(n) }];
  assert(JSON.stringify(computed) === '[{"id":1},{"id":2,"v":4},{"id":3,"v":0,"w":"3"}]', "computed field values");

  const arities = [{ f: (x: number) => x }, { f: (x: number, y: number) => x * y }];
  assert(arities.map((o) => o.f(3, 4)).join(",") === "3,12", "same fields join by type before normalizing");

  const missing: { foo?: string } | null = flag ? opts : null;
  const fallback = missing ?? {};
  assert(show(fallback.foo ?? null) === "null", "`??` joins `{}` too");

  const wider = [{ p: { x: 1, y: 2 } }, { p: { x: 3 } }];
  assert(wider.map((v) => show(v.p.y ?? null)).join(",") === "2,null", "nested fields differ under the same top-level fields");
  const narrower = [{ p: { a: 1 }, q: 1 }, { p: { a: 2, b: 5 }, q: 2 }];
  assert(narrower.map((v) => show(v.p.b ?? null)).join(",") === "null,5", "a later nested literal adds a field");

  const crossed = [{ k: 1, s: "a" }, { k: "b", s: 2 }];
  assert(JSON.stringify(crossed) === '[{"k":1,"s":"a"},{"k":"b","s":2}]', "same fields, neither element fits the other");

  const emptyFirst = [{ p: {} }, { p: { x: 1 } }];
  assert(emptyFirst.map((v) => show(v.p.x ?? null)).join(",") === "null,1", "a nested empty object literal");
  const alternating = [{ p: { x: 1 } }, { p: { x: 2, y: 2 } }, { p: { x: 3 } }, { p: { z: 5 } }];
  assert(
    alternating.map((v) => show(v.p.x ?? null) + show(v.p.y ?? null) + show(v.p.z ?? null)).join(",") ===
      "1nullnull,22null,3nullnull,nullnull5",
    "nested shapes alternating after the element type became a union",
  );
}
