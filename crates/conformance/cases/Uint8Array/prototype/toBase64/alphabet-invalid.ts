// Divergence pin, derived from test262: test/built-ins/Uint8Array/prototype/toBase64/alphabet.js
// (its last row passes { alphabet: 'other' } and expects a TypeError at
// runtime). alphabet is typed "base64" | "base64url", so the bad literal is
// rejected at compile time instead.
// expect-error: expected `"base64" | "base64url"`, got `"other"`

function main(): void {
  const s: string = new Uint8Array([199, 239, 242]).toBase64({ alphabet: "other" });
  assert(s === "", "unreachable");
}
