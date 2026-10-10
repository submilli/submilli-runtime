// expect-error: method `scale` expects 1 argument(s), got 2
// expect-error: method `scale` expects 1 argument(s), got 0
// expect-error: expected `number`, got `string`
// expect-error: expected 2 argument(s), got 1
// expect-error: expected 1 argument(s), got 0
// A `Call` step in an optional chain checks arity and argument types against
// the resolved signature, the same as the non-chain call it desugars to.
// Accepting these produced a Wasm validation failure or a codegen panic with
// no source location — the worst failure shape for a generated program.
type Adder = (a: number, b: number) => number;

class Scaler {
  scale(by: number): number {
    return by * 2;
  }
}

interface Formatter {
  fmt: (n: number) => string;
}

function main(): void {
  const s: Scaler | null = new Scaler();
  const tooMany = s?.scale(1, 2);
  const tooFew = s?.scale();
  const wrongType = s?.scale("nope");

  const add: Adder | null = (a: number, b: number): number => a + b;
  const closureTooFew = add?.(1);

  const f: Formatter | null = { fmt: (n: number): string => n.toString() };
  const propertyTooFew = f?.fmt();

  assert(tooMany === null, "");
  assert(tooFew === null, "");
  assert(wrongType === null, "");
  assert(closureTooFew === null, "");
  assert(propertyTooFew === null, "");
}
