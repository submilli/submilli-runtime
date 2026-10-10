// `x === false` (or any non-nullish literal) rules out `null` and `undefined`
// in its true branch, as in tsc: neither equals the literal.
function isOff(a?: boolean): string {
  if (a === false) {
    const off: false = a;
    return `off:${off}`;
  }
  return "other";
}

function isOne(n: number | null): number {
  if (n === 1) {
    const one: 1 = n;
    return one;
  }
  return 0;
}

function main(): void {
  assert(isOff(false) === "off:false" && isOff() === "other" && isOff(true) === "other", "an optional boolean");
  assert(isOne(1) === 1 && isOne(null) === 0, "a nullable number");
}
