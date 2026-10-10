// Only fresh object literals normalize, as in tsc: a value typed with fewer
// fields may hold the others, with any type, so reading them through a
// normalized union could read the wrong type. The same holds one level down,
// and for a `{}`-typed value joined with an all-optional object.
// expect-error: field `b` does not exist on all members of `{ a: number } | { a: number; b: number }` (missing on `{ a: number }`)
// expect-error: field `x` does not exist on all members of `{ a: number } | { x: number }` (missing on `{ a: number }`)
// expect-error: field `foo` does not exist on all members of `{} | { foo?: string }` (missing on `{}`)
// expect-error: field `c` does not exist on all members of `{ a: number } | { a: number; c: number }` (missing on `{ a: number }`)
// expect-error-count: 4
function main(): void {
  const source = { a: 3, b: "s", x: "s", foo: 42, c: "s" };
  const short: { a: number } = source;
  const withShort = [{ a: 1 }, { a: 2, b: 2 }, short];
  const b = withShort[0].b;

  const nested = [{ p: short }, { p: { x: 1 } }];
  const x = nested[0].p.x;

  const empty: {} = source;
  const opts: { foo?: string } = {};
  const joined = opts.foo === null ? empty : opts;
  const foo = joined.foo;

  const spread = [{ ...short }, { ...short, c: 2 }];
  const c = spread[1].c;
}
