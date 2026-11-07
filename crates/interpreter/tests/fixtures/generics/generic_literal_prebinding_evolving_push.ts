// A later argument that grows an evolving array still binds a type parameter
// early for the callback in the literal argument before it, as in tsc. Only the
// array-literal retry counts the growth as a write (SUB-2121).
function pick<T>(o: { f: (x: T) => T }, v: T): T {
  return o.f(v);
}

function main(): void {
  let xs = [];
  xs.push(1);
  const r = pick({ f: (x) => x + 1 }, xs.push(2));
  assert(r === 3 && xs.length === 2, "the push binds T");
}
