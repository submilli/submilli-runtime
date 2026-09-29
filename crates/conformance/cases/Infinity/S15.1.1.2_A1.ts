// test262: test/built-ins/Infinity/S15.1.1.2_A1.js

function main(): void {
  assert(typeof Infinity === "number", 'The value of `typeof(Infinity)` is expected to be "number"');
  assertSameValue(isFinite(Infinity), false, "isFinite(Infinity) must return false");
  assertSameValue(isNaN(Infinity), false, "isNaN(Infinity) must return false");

  assertSameValue(
    Infinity,
    Number.POSITIVE_INFINITY,
    "The value of Infinity is expected to equal the value of Number.POSITIVE_INFINITY",
  );
}
