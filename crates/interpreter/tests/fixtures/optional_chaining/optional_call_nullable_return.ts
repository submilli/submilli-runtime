// `?.()` on a closure whose own return type is nullable: the `| null` belongs
// to the function's signature, not to the chain's short-circuit, so the call's
// result must stay in its nullable slot rather than being unboxed.

type Fn = (x: number) => number | null;

function positive(x: number): number | null {
  if (x > 0) {
    return x + 2;
  }
  return null;
}

function callIt(f: Fn | null, x: number): number | null {
  return f?.(x);
}

function main(): void {
  assert(callIt(positive, 1) === 3, "non-null result flows through");
  assert(callIt(positive, -1) === null, "the closure's own null flows through");
  assert(callIt(null, 1) === null, "null callee short-circuits");
}
