// A generic value passed where the call is still inferring its own type
// parameters has no target to instantiate against yet (tsc infers `number`);
// explicit type arguments work.
// expect-error: pass the call's type arguments explicitly
// expect-error-count: 1
function apply<T>(f: (v: T) => T, v: T): T {
  return f(v);
}
const id = <U,>(x: U): U => x;

function main(): void {
  const ok: number = apply<number>(id, 4);
  const inferred = apply(id, 4);
}
