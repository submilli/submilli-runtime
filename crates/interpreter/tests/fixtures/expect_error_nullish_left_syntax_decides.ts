// The left side of `??` whose syntax decides whether it is nullish wastes one
// operand, as tsc reports: never nullish (TS2869) leaves the right unreachable,
// always nullish (TS2871) the left. A read, call or logical operator can be
// either, so it isn't judged.
// expect-error: right operand of `??` is unreachable because the left operand is never nullish
// expect-error: this expression is always nullish
// expect-error-count: 4
function main(): void {
  const n: number | null = null as number | null;
  const a = 5 ?? 3;
  const b = `x${n}` ?? "y";
  const c = (n === null) ?? false;
  const d = null ?? 1;
  const e = n ?? 1;
  console.log(a, b, c, d, e);
}
