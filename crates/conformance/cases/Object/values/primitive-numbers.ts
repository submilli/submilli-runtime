// test262: test/built-ins/Object/values/primitive-numbers.js

function main(): void {
  assertSameValue(Object.values(0).length, 0, "0 has zero values");
  assertSameValue(Object.values(-0).length, 0, "-0 has zero values");
  assertSameValue(Object.values(Infinity).length, 0, "Infinity has zero values");
  assertSameValue(Object.values(-Infinity).length, 0, "-Infinity has zero values");
  assertSameValue(Object.values(NaN).length, 0, "NaN has zero values");
  assertSameValue(Object.values(Math.PI).length, 0, "Math.PI has zero values");
}
