// A function literal called on the spot returns into its call's context, as
// in tsc: returns that share no one type join into the expected type, and an
// object literal it returns still lets a generic call infer from it.
interface Box<T> {
  v: T;
}

function unbox<T>(b: Box<T>): T {
  return b.v;
}

function main(): void {
  const n: number | null = (() => {
    if (Math.random() > 2) {
      return null;
    }
    return 1;
  })();
  assert(n === 1, "returns joined by the context");
  const text = unbox((() => ({ v: "x" }))());
  assert(text.length === 1, "an object literal for a generic interface");
  const count = unbox((() => {
    return { v: 5 };
  })());
  assert(count + 1 === 6, "a block body's object literal");
}
