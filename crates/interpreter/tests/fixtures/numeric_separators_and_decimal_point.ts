function main(): void {
  // A single `_` between digits, in every digit run.
  assert(1_000_000 === 1000000, "integer");
  assert(1_000.25_5 === 1000.255, "integer and fraction");
  assert(1e1_0 === 1e10, "exponent");
  assert(0xFF_FF === 65535 && 0b1010_1010 === 170 && 0o7_7 === 63, "radix digits");
  assert(1_000n === 1000n && 0xF_Fn === 255n, "bigint");

  // A literal may end in `.`, so a second `.` reads a member.
  assert(1. === 1 && 1.e3 === 1000, "trailing decimal point");
  assert(1..toString() === "1" && 0..toFixed(1) === "0.0", "member after `1.`");
  assert((1).toString() === "1" && 1.5.toString() === "1.5", "parenthesized and fractional");
}
