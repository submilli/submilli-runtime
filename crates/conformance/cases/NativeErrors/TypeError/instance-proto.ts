// test262: test/built-ins/NativeErrors/TypeError/instance-proto.js
//
// No prototypes: "the instance's prototype is TypeError.prototype" becomes the
// nominal check that reaches the same fact, `instanceof TypeError`. The value is
// widened to `unknown` so the check is dynamic, not a compile-time tautology.
// `new TypeError` becomes `new TypeError("")` (the constructor requires a message).

function main(): void {
  const error: unknown = new TypeError("");
  assertSameValue(error instanceof TypeError, true);
}
