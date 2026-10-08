// Before normalizing, object literals reduce to those no other is a subtype of,
// as in tsc: `{ a: { b: never[] } }` goes beside `{ a: { b: number[] } }`, so
// `b` is a writable `number[]`, while `{ a: {} }` stays, as the other has a
// field it lacks.
function main(): void {
  const xs = [{ a: {} }, { a: { b: [] } }, { a: { b: [1] } }];
  const box = xs[2].a.b;
  if (box) {
    box.push(5);
    assert(box.join(",") === "1,5", "the empty array joins the numbers");
  }
  assert(xs[0].a.b == null, "the empty object reads b as missing");

  const o = { p: 1 };
  const withExtra = [o, { p: 2, q: 3 }];
  const second = withExtra[1];
  assert("q" in second && second.q === 3, "a literal with an extra field keeps its own type");
}
