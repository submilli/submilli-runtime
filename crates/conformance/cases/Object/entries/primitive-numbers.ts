// test262: test/built-ins/Object/entries/primitive-numbers.js

function main(): void {
  assertSameValue(Object.entries(0).length, 0, "0 has zero entries");
  assertSameValue(Object.entries(-0).length, 0, "-0 has zero entries");
  assertSameValue(Object.entries(Infinity).length, 0, "Infinity has zero entries");
  assertSameValue(Object.entries(-Infinity).length, 0, "-Infinity has zero entries");
  assertSameValue(Object.entries(NaN).length, 0, "NaN has zero entries");
  assertSameValue(Object.entries(Math.PI).length, 0, "Math.PI has zero entries");
}
