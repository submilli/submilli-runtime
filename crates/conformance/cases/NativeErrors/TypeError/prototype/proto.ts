// test262: test/built-ins/NativeErrors/TypeError/prototype/proto.js
//
// No prototypes: "TypeError.prototype inherits from Error.prototype" becomes
// the nominal check that an instance is an `Error`, and that catching as
// `Error` binds it (spec.md §1.8). The value is widened to `unknown` so the
// `instanceof` is dynamic.

function main(): void {
  const error: unknown = new TypeError("");
  assertSameValue(error instanceof Error, true);

  let caughtName = "";
  try {
    throw new TypeError("thrown");
  } catch (e: Error) {
    caughtName = e.name;
  }
  assertSameValue(caughtName, "TypeError", "catch (e: Error) binds a TypeError");
}
