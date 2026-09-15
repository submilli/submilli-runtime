// test262: test/built-ins/Uint8Array/prototype/toBase64/results.js

function main(): void {
  // standard test vectors from https://datatracker.ietf.org/doc/html/rfc4648#section-10
  assertSameValue(new Uint8Array([]).toBase64(), "");
  assertSameValue(new Uint8Array([102]).toBase64(), "Zg==");
  assertSameValue(new Uint8Array([102, 111]).toBase64(), "Zm8=");
  assertSameValue(new Uint8Array([102, 111, 111]).toBase64(), "Zm9v");
  assertSameValue(new Uint8Array([102, 111, 111, 98]).toBase64(), "Zm9vYg==");
  assertSameValue(new Uint8Array([102, 111, 111, 98, 97]).toBase64(), "Zm9vYmE=");
  assertSameValue(new Uint8Array([102, 111, 111, 98, 97, 114]).toBase64(), "Zm9vYmFy");
}
