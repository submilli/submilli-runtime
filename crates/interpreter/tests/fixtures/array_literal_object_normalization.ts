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
}
