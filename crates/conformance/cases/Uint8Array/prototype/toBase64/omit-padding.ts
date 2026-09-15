// test262: test/built-ins/Uint8Array/prototype/toBase64/omit-padding.js
// The ToBoolean rows ({ omitPadding: 0 } / { omitPadding: 1 }) are dropped:
// omitPadding is typed boolean and numbers never coerce.

function main(): void {
  // works with default alphabet
  assertSameValue(new Uint8Array([199, 239]).toBase64(), "x+8=");
  assertSameValue(new Uint8Array([199, 239]).toBase64({ omitPadding: false }), "x+8=");
  assertSameValue(new Uint8Array([199, 239]).toBase64({ omitPadding: true }), "x+8");
  assertSameValue(new Uint8Array([255]).toBase64({ omitPadding: true }), "/w");

  // works with base64url alphabet
  assertSameValue(new Uint8Array([199, 239]).toBase64({ alphabet: "base64url" }), "x-8=");
  assertSameValue(new Uint8Array([199, 239]).toBase64({ alphabet: "base64url", omitPadding: false }), "x-8=");
  assertSameValue(new Uint8Array([199, 239]).toBase64({ alphabet: "base64url", omitPadding: true }), "x-8");
  assertSameValue(new Uint8Array([255]).toBase64({ alphabet: "base64url", omitPadding: true }), "_w");
}
