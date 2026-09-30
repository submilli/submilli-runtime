// test262: test/built-ins/NativeErrors/SyntaxError/prototype/name.js
//
// No prototypes: the value of SyntaxError.prototype.name is read through an
// instance, where it is the `name` field's default. The descriptor checks
// are dropped (blanket rule).

function main(): void {
  const error = new SyntaxError("m");
  assertSameValue(error.name, "SyntaxError");
  assertSameValue(error.toString(), "SyntaxError: m");
}
