// test262: test/built-ins/Number/prototype/toPrecision/undefined-precision-arg.js
// Adapted: `new Number(7)` becomes the primitive 7 (no wrapper objects);
// `Number.prototype.toPrecision()` (receiver +0) becomes `(0).toPrecision()`.

function main(): void {
  const n = 7;

  assertSameValue(n.toPrecision(undefined), "7");
  assertSameValue((39).toPrecision(undefined), "39");

  assertSameValue((0).toPrecision(), "0");
  assertSameValue((42).toPrecision(), "42");
}
