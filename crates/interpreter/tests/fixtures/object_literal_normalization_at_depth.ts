// Sibling object literals normalize at every depth, as in tsc: a field's
// siblings are the objects it holds in the other literals, leaving out `null`
// and primitives, so each field any of them names reads from the union.
function main(): void {
  const xs = [{ k: 2, p: { x: 1 } }, { k: 1, p: null }, { k: 3, p: { y: "s" } }];
  const read = xs.map((v) => (v.p ? String(v.p.x ?? "-") + String(v.p.y ?? "-") : "none"));
  assert(read.join(",") === "1-,none,-s", "objects beside null normalize");

  const deep = [{ a: { b: { c: 1 } } }, { a: { b: { d: "x" } } }];
  assert(deep[0].a.b.d == null && deep[1].a.b.c == null, "two levels down normalize");
  assert(deep[0].a.b.c === 1 && deep[1].a.b.d === "x", "declared fields keep their values");

  const mixed = [{ p: { x: 1 } }, { p: 5 }, { p: { y: 2 } }];
  let sum = 0;
  for (const m of mixed) {
    sum += typeof m.p === "number" ? m.p : (m.p.x ?? 0) + (m.p.y ?? 0);
  }
  assert(sum === 8, "objects beside a primitive normalize");

  const shapes = [{ a: { x: 1, y: 2 } }, { a: { x: 1 } }];
  assert(shapes[1].a.y == null, "a missing nested field reads as missing");
}
