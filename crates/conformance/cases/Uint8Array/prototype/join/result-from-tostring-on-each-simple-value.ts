// test262: test/built-ins/TypedArray/prototype/join/result-from-tostring-on-each-simple-value.js
// Instantiated at Uint8Array.

function main(): void {
  const sample: Uint8Array = new Uint8Array([1, 0, 2, 3, 42, 127]);
  const result: string = sample.join();
  assertSameValue(result, "1,0,2,3,42,127");
}
