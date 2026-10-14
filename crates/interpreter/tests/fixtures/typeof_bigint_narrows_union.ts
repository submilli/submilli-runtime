// `typeof x === "bigint"` narrows a union to its bigint member, and the else
// branch to the rest.
function describe(x: bigint | number): string {
  if (typeof x === "bigint") {
    return (x * 2n).toString();
  }
  return x.toFixed(1);
}

function wider(x: bigint | string | null): number {
  if (typeof x !== "bigint") {
    return x === null ? -1 : x.length;
  }
  return Number(x + 1n);
}

function main(): void {
  assert(describe(10n) === "20");
  assert(describe(2) === "2.0");
  assert(wider(4n) === 5);
  assert(wider("abc") === 3);
  assert(wider(null) === -1);
}
