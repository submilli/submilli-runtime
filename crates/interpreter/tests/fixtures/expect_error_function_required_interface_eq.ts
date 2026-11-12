// Comparing a plain function against an interface with required non-call
// properties remains a compile-time error because no value satisfies both.
// expect-error: expected `RequiredMeta`, got `(arg0: number) => number`
// expect-error-count: 1
interface RequiredMeta {
  tag: string;
}

function main(): boolean {
  const fn = (n: number): number => n * 2;
  const meta: RequiredMeta = { tag: "ok" };
  return meta === fn;
}
