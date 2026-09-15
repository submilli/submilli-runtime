// test262: test/built-ins/TypedArray/prototype/join/empty-instance-empty-string.js
// Instantiated at Uint8Array (new TA(0) → Uint8Array.alloc(0)).

function main(): void {
  const sample: Uint8Array = Uint8Array.alloc(0);
  assertSameValue(sample.join(), "");
  assertSameValue(sample.join("test262"), "");
}
