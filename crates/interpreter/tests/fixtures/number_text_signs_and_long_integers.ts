function main(): void {
  // `Number` of a text takes at most one sign.
  for (const text of ["++1", "--1", "+-1", "-+1", "+-Infinity"]) {
    assert(Number.isNaN(Number(text)), `Number("${text}") is NaN`);
  }
  assert(Number("+1") === 1, "one plus sign");
  assert(Number("-1") === -1, "one minus sign");
  assert(Number("-Infinity") === -Infinity, "a sign before Infinity");

  // `parseInt` of a long digit string is the double nearest to it.
  assert(parseInt("12345678901234567890") === 12345678901234567000, "20 digits");
  assert(parseInt("123456789012345678901234567890") === 1.2345678901234568e29, "30 digits");
  assert(
    parseInt("1234567890123456789012345678901234567890") === 1.2345678901234568e39,
    "40 digits",
  );
  assert(parseInt("-123456789012345678901234567890") === -1.2345678901234568e29, "a sign");
  assert(parseInt("200000000000011", 16) === 144115188075855900, "hexadecimal digits");
  assert(Number("0x200000000000011") === 144115188075855900, "a hexadecimal literal");
}
