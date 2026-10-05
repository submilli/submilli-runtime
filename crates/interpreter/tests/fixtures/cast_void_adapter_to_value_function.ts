// A value-returning function stored as a `void` function type still returns its
// value, so a cast back through `unknown` to a value-returning type succeeds at
// its own arity or a larger one, and a value function casts to a `void` type as
// it assigns to one. A function declared `void` has no value to give, so a cast
// to a value type rejects it, also when a generic stored it as a value function.
type Unary = (a: number) => number;
type Binary = (a: number, b: number) => number;
type Ternary = (a: number, b: number, c: number) => number;

function rejects(cast: () => void): boolean {
  try {
    cast();
  } catch (e: TypeError) {
    return e.message.includes("type mismatch");
  }
  return false;
}

function keep<T>(f: (a: number) => T): unknown {
  return f;
}

function main(): void {
  const times7 = (a: number): number => a * 7;
  const dropped: (a: number) => void = times7;
  assert(((dropped as unknown) as Unary)(2) === 14, "same arity");
  assert(((dropped as unknown) as Binary)(3, 0) === 21, "larger arity");

  const sum = (a: number, b: number, c: number): number => a + b + c;
  const droppedSum: (a: number, b: number, c: number) => void = sum;
  assert(((droppedSum as unknown) as Ternary)(1, 2, 3) === 6, "three parameters");

  const nothing = (a: number): void => {};
  assert(rejects(() => { const f = (nothing as unknown) as Unary; }), "declared void");
  assert(rejects(() => { const f = keep(nothing) as Unary; }), "declared void through a generic");

  let total = 0;
  const record: unknown = (a: number): number => {
    total += a;
    return total;
  };
  (record as (a: number) => void)(5);
  assert(total === 5, "value function to a void type");
}
