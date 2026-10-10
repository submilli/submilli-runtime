// A type whose fields are all optional takes only a value that shares at least
// one field with it, as in TypeScript (TS2559). Otherwise width subtyping would
// let `T3.q` read the number `T1` put there as a string.
// expect-error-count: 6
// expect-error: expected `T3`, got `T2`
// expect-error: expected `T3`, got `T2`
// expect-error: expected `T3`, got `T2`
// expect-error: expected `T3`, got `T2`
// expect-error: expected `T3`, got `T2`
// expect-error: expected `{ inner: T3 }`
type T1 = { p: number; q: number };
type T2 = { p: number };
type T3 = { q?: string };

function take(t: T3): string {
  return t.q ?? "none";
}

function give(d: T2): T3 {
  return d;
}

function main(): void {
  const c: T1 = { p: 1, q: 2 };
  const d: T2 = c;
  const e: T3 = d;
  console.log(take(d), give(d), e);
  const list: T3[] = [d];
  const holder: { inner: T3 } = { inner: d };
  console.log(list, holder);
}
