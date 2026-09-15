// test262: test/built-ins/Uint8Array/prototype/toBase64/alphabet.js
// The invalid-alphabet row is statically rejected here; pinned by
// alphabet-invalid.ts.

function main(): void {
  assertSameValue(new Uint8Array([199, 239, 242]).toBase64(), "x+/y");
  assertSameValue(new Uint8Array([199, 239, 242]).toBase64({ alphabet: "base64" }), "x+/y");
  assertSameValue(new Uint8Array([199, 239, 242]).toBase64({ alphabet: "base64url" }), "x-_y");
}
