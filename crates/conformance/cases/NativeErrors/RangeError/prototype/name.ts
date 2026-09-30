// test262: test/built-ins/NativeErrors/RangeError/prototype/name.js
//
// No prototypes: the value of RangeError.prototype.name is read through an
// instance, where it is the `name` field's default. The descriptor checks
// are dropped (blanket rule).

function main(): void {
  const error = new RangeError("m");
  assertSameValue(error.name, "RangeError");
  assertSameValue(error.toString(), "RangeError: m");
}
