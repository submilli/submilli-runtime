// An assertion on a generic function narrows to the instantiated type.
function assertNotNull<T>(x: T | null): asserts x is T {
  if (x === null) throw new Error("null");
}

function main(): void {
  const s: string | null = "abc";
  assertNotNull(s);
  assert(s.length === 3, "null removed through a generic assertion");
  const o: { n: number } | null = { n: 4 };
  assertNotNull(o);
  assert(o.n === 4, "object kept");
}
