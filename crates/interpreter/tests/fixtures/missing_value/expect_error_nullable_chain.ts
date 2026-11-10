// SUB-2148: a `?.` on a receiver that may be missing and whose type also
// allows `null` can short-circuit on the `null`, so the chain is
// `T | undefined`, as tsc types it.
// expect-error: expected `Inner`, got `Inner | undefined`
interface Inner {
  z: number;
}
interface Item {
  sub: Inner;
}

function main(): void {
  const xs: (Item | null)[] = [null];
  const inner: Inner = xs[0]?.sub;
  console.log(inner.z);
}
