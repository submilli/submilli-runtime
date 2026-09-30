// test262: test/built-ins/NativeErrors/SyntaxError/instance-proto.js
//
// No prototypes: "the instance's prototype is SyntaxError.prototype" becomes the
// nominal check that reaches the same fact, `instanceof SyntaxError`. The value is
// widened to `unknown` so the check is dynamic, not a compile-time tautology.
// `new SyntaxError` becomes `new SyntaxError("")` (the constructor requires a message).

function main(): void {
  const error: unknown = new SyntaxError("");
  assertSameValue(error instanceof SyntaxError, true);
}
