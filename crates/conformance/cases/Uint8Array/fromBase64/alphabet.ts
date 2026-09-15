// test262: test/built-ins/Uint8Array/fromBase64/alphabet.js
// The runtime throws SyntaxError here; assertThrows matches it via the base Error.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(Uint8Array.fromBase64("x+/y"), [199, 239, 242], "default alphabet");
  assertBytes(Uint8Array.fromBase64("x+/y", { alphabet: "base64" }), [199, 239, 242], "explicit base64");
  assertThrows((): void => {
    Uint8Array.fromBase64("x+/y", { alphabet: "base64url" });
  }, "'+'/'/' rejected under base64url");

  assertBytes(Uint8Array.fromBase64("x-_y", { alphabet: "base64url" }), [199, 239, 242], "base64url");
  assertThrows((): void => {
    Uint8Array.fromBase64("x-_y");
  }, "'-'/'_' rejected under default alphabet");
  assertThrows((): void => {
    Uint8Array.fromBase64("x-_y", { alphabet: "base64" });
  }, "'-'/'_' rejected under explicit base64");
}
