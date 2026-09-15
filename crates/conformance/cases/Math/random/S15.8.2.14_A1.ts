// test262: test/built-ins/Math/random/S15.8.2.14_A1.js
// The typeof-result assertion is subsumed by the static number type.

function main(): void {
  for (let i = 0; i < 100; i++) {
    const val: number = Math.random();
    assertNotSameValue(val, NaN, "should not produce NaN");
    assert(!(val < 0 || val >= 1), `#1: Math.random() = ${val}`);
  }
}
