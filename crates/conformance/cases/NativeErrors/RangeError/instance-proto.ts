// test262: test/built-ins/NativeErrors/RangeError/instance-proto.js
//
// No prototypes: "the instance's prototype is RangeError.prototype" becomes the
// nominal check that reaches the same fact, `instanceof RangeError`. The value is
// widened to `unknown` so the check is dynamic, not a compile-time tautology.
// `new RangeError` becomes `new RangeError("")` (the constructor requires a message).

function main(): void {
  const error: unknown = new RangeError("");
  assertSameValue(error instanceof RangeError, true);
}
