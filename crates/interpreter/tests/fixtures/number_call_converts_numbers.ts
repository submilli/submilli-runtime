// SUB-2102: `Number(value)` also takes a number, returned as it is, and can be
// called with no argument, which gives 0. An explicit `undefined` stays `NaN`,
// as in JavaScript.
function convert(value: number | string): number {
  return Number(value);
}
function main(): void {
  const n: number = 5;
  assert(Number(n) === 5);
  assert(Object.is(Number(-0), -0) && isNaN(Number(NaN)));
  assert(Number(Infinity) === Infinity && Number(-Infinity) === -Infinity);
  assert(Number(Number.MAX_VALUE) === Number.MAX_VALUE);
  assert(Number(Number.MIN_VALUE) === Number.MIN_VALUE);
  assert(Number() === 0);
  assert(isNaN(Number(undefined)));
  assert(convert(5) === 5 && convert("7") === 7 && isNaN(convert("x")));
  const missing: number | undefined = undefined;
  const present: number | undefined = 3 as number | undefined;
  assert(isNaN(Number(missing)) && Number(present) === 3);
  const any: number | string | bigint | undefined = 12n as number | string | bigint | undefined;
  assert(Number(any) === 12);
  const four: 4 = 4;
  assert(Number(four) === 4);
  assert([1, 2].map((x) => Number(x)).join(",") === "1,2");
}
