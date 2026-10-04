const none: number | null = null;
const zero: number | null = 0;

function main(): void {
  // `??` beside `||` / `&&` with explicit grouping, and beside operators that need none.
  assert(((none ?? zero) || 3) === 3, "(a ?? b) || c");
  assert((none ?? (zero || 3)) === 3, "a ?? (b || c)");
  assert(((none || zero) ?? 3) === 0, "(a || b) ?? c");
  assert((none || (zero ?? 3)) === 0, "a || (b ?? c)");
  assert(((none ?? zero) && 3) === 0, "(a ?? b) && c");
  assert((none ?? zero ?? 3) === 0, "chained ??");
  assert((none ?? (zero ?? 3)) === 0, "nested ??");
  assert((none ?? zero === 0) === true, "?? beside ===");
  assert((none ?? zero ? 1 : 2) === 2, "?? beside a ternary");
  assert((none ?? [zero || 3][0]) === 3, "|| inside brackets");
  assert((none ?? `${zero || 3}`) === "3", "|| inside a template");

  // A lone zero, and a zero that only starts a fraction, exponent, or radix prefix.
  assert(0 === 0 && 0.5 * 2 === 1, "zero and a zero-led fraction");
  assert(0e1 === 0 && 0.010 === 0.01, "zero exponent, zeros after the point");
  assert(1e010 === 1e10, "leading zero in an exponent");
  assert(0x010 === 16 && 0o17 === 15 && 0b1 === 1, "radix prefixes");
  assert(0n === BigInt(0), "bigint zero");

  // Number-like text in strings and templates is not a literal.
  assert("010".length === 3 && `08.5` === "08.5", "text stays text");
  assert(Number("010") === 10, "string conversion is decimal");
}
