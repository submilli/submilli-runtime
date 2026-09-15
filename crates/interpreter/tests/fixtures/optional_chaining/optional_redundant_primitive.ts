// expect-warning: optional chain on non-nullable receiver
// A `?.` whose receiver is a non-nullable `number`/`boolean` is redundant — a
// warning, not an error. Its slot is an f64/i32, which `ref.is_null` does not
// accept, so the step has to lower as a straight-line access with no
// short-circuit branch at all.
interface Holder {
  n: number;
}

function main(): void {
  const x: number = 1.5;
  assert(x?.toFixed(1) === "1.5", "redundant `?.` on a number");

  const b: boolean = true;
  assert(b?.toString() === "true", "redundant `?.` on a boolean");

  // same when the primitive receiver arrives mid-chain from a field step
  const h: Holder = { n: 2.5 };
  assert(h.n?.toFixed(1) === "2.5", "redundant `?.` after a field step");

  // ...including nested inside a short-circuit an earlier step opened, where
  // the straight-line access has to leave the enclosing block's result type
  // intact
  const maybe: Holder | null = { n: 2.5 };
  assert(maybe?.n?.toFixed(1) === "2.5", "straight-line step inside a short-circuit");
  assert(maybe?.n?.toFixed(1)?.length === 3, "and a further step after it");

  const absent: Holder | null = null;
  assert(absent?.n?.toFixed(1) === null, "outer short-circuit still wins");

  // a non-nullable *ref* receiver keeps the short-circuit shape and still works
  const s: string = "abc";
  assert(s?.length === 3, "redundant `?.` on a string");
}
