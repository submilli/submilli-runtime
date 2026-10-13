// A field written before a spread that always sets it is dead code: the spread
// overwrites it whatever its value, as tsc reports (TS2783). A spread field
// that may be absent, because it is optional or one alternative lacks it, keeps
// the earlier value in place and is fine.
// expect-error: `a` is specified more than once, so this value is always overwritten
// expect-error: `b` is specified more than once, so this value is always overwritten
// expect-error-count: 2
type Point = {
  a: number;
  b?: number;
};

function main(): void {
  const point: Point = { a: 1 };
  const flag: boolean = point.a > 0;
  const dead = { a: 0, ...point };
  const kept = { b: 0, ...point };
  const after = { ...point, a: 2 };
  const maybe = { a: 0, ...(flag ? point : {}) };
  const unionOfBoth = { b: 0, ...(flag ? { b: 1 } : { b: 2 }) };
  console.log(dead.a, kept.b, after.a, maybe.a, unionOfBoth.b);
}
