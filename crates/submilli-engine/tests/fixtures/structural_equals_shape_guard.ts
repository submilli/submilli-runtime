// Structural `===` across different shapes met via `unknown`: the equals body
// must guard on field names and arity — same-arity different names compare
// unequal (not slot-blind equal), and a shorter RHS payload returns false
// instead of trapping out-of-bounds.
function main(): void {
  const a: unknown = { a: 1 };
  const b: unknown = { b: 1 };
  assert(a !== b);
  assert(b !== a);
  assert(!(a === b));

  const wide: unknown = { a: 1, b: 2 };
  assert(wide !== a);
  assert(a !== wide);

  const a2: unknown = { a: 1 };
  assert(a === a2);
  assert(!(a !== a2));

  const p1: unknown = { x: 1, y: 2 };
  const p2: unknown = { y: 2, x: 1 };
  assert(p1 === p2);

  const q: unknown = { x: 1, z: 2 };
  assert(p1 !== q);
  assert(q !== p1);

  const empty: unknown = {};
  assert(empty !== a);
  assert(a !== empty);
  const empty2: unknown = {};
  assert(empty === empty2);

  // Same field names, different field types: unequal, not a cast trap.
  const numX: unknown = { x: 1 };
  const strX: unknown = { x: "1" };
  assert(numX !== strX);
  assert(strX !== numX);

  const boolX: unknown = { x: true };
  assert(numX !== boolX);
  assert(boolX !== numX);

  const nullX: unknown = { x: null };
  assert(numX !== nullX);
  assert(nullX !== numX);
  const nullX2: unknown = { x: null };
  assert(nullX === nullX2);

  const objX: unknown = { x: { y: 1 } };
  assert(numX !== objX);
  assert(objX !== numX);
}
