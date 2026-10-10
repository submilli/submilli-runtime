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
  assert(xs[0].a.b === undefined, "the empty object reads b as missing");

  const o = { p: 1 };
  const withExtra = [o, { p: 2, q: 3 }];
  const second = withExtra[1];
  assert("q" in second && second.q === 3, "a literal with an extra field keeps its own type");

  // A literal that only spreads isn't checked for excess fields, but still
  // lacks an optional field the way any object literal may.
  const withOptional: { p: number; q?: number } = { p: 4, q: 5 };
  const spread = [withOptional, { ...o }];
  assert(spread[0].q === 5 && spread[1].q === undefined, "the spread joins the type with the optional field");

  // A non-empty object literal target keeps a source with a field it lacks,
  // spread or not; an empty `{}` absorbs a spread.
  const oz = { x: 1, z: 9 };
  const spreadFirst = [{ ...oz }, { x: 1 }];
  const kept = spreadFirst[0];
  assert("z" in kept && kept.z === 9, "the spread keeps its extra field");
  const spreadSecond = [{ x: 1 }, { ...oz }];
  const keptSecond = spreadSecond[1];
  assert("z" in keptSecond && keptSecond.z === 9, "the later spread keeps its extra field");
  const intoEmpty = [{ ...o }, {}];
  assert(intoEmpty.length === 2, "a spread joins an empty literal");
}
