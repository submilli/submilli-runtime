// expect-error: field `a1` does not exist on `{}`
// As in tsc, a spread keeps an index signature only where every source has one
// of the same key, so spreading records of different domains leaves none.
function main(): void {
  const a: Record<`a${string}`, number> = { a1: 1 };
  const b: Record<`b${string}`, string> = { b1: "x" };
  const result = { ...a, ...b };
  const n: number = result.a1;
}
