// test262: test/built-ins/NativeErrors/TypeError/prototype/name.js
//
// No prototypes: the value of TypeError.prototype.name is read through an
// instance, where it is the `name` field's default. The descriptor checks
// are dropped (blanket rule).

function main(): void {
  const error = new TypeError("m");
  assertSameValue(error.name, "TypeError");
  assertSameValue(error.toString(), "TypeError: m");
}
