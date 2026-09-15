// test262: test/built-ins/Uint8Array/fromHex/illegal-characters.js
// The runtime throws SyntaxError here; assertThrows matches it via the base Error.

function main(): void {
  const illegal: string[] = [
    "a.a",
    "aa^",
    "a a",
    "a\ta",
    "a\na",
    "a\fa",
    "a\ra",
    "a\u{00A0}a", // nbsp
    "a\u{2009}a", // thin space
    "a\u{2028}a", // line separator
  ];
  for (const value of illegal) {
    assertThrows((): void => {
      Uint8Array.fromHex(value);
    }, "fromHex with illegal character");
  }
}
