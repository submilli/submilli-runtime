// `Object.assign` copies values: a function a later source writes over an
// object field replaces it, so the field is the function, not a hybrid of both.
// TypeScript types it `{ tag: string } & F` and accepts the read, which yields
// `undefined`; Submilli types it as what the field holds.
// expect-error: cannot read field `tag` on non-object type `(arg0: number) => number`
// expect-error: cannot read field `x` on non-object type `((arg0: number) => number) | { x: number }`
// expect-error-count: 2
function main(): void {
  const merged = Object.assign({ f: { tag: "a" } }, { f: (n: number): number => n + 1 });
  assert(merged.f(1) === 2);
  const tag: string = merged.f.tag;
  // An optional later source may leave the earlier function in place.
  const absent: { f?: { x: number } } = {};
  const kept = Object.assign({}, { f: (n: number): number => n }, absent);
  if (kept.f !== undefined) {
    const x: number = kept.f.x;
  }
}
