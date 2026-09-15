// test262: test/built-ins/Math/clz32/Math.clz32.js

function main(): void {
  assertSameValue(Math.clz32(0), 32, "Math.clz32(0)");
  assertSameValue(Math.clz32(-0), 32, "Math.clz32(-0)");
}
