// A ternary joining a literal-typed `const` with a base-typed value is the base, as
// in TypeScript — not `number | 10`, which would have no arithmetic at all. Found by
// `submilli build test`: this shape appears in the maintained `notion` package.
const DEFAULT_CHUNK = 10;

function chosen(override: number | null): number {
  return override === null ? DEFAULT_CHUNK : override;
}

function main(): void {
  const a = chosen(null);
  const b = chosen(25);
  // The point of the widening: the result still supports arithmetic.
  assert(a / 2 === 5, "default branch divides");
  assert(b - 5 === 20, "override branch subtracts");
}
