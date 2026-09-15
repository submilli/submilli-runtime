// test262: test/built-ins/isFinite/return-false-on-nan-or-infinities.js

function main(): void {
  assertSameValue(isFinite(NaN), false, "NaN");
  assertSameValue(isFinite(Infinity), false, "Infinity");
  assertSameValue(isFinite(-Infinity), false, "-Infinity");
}
