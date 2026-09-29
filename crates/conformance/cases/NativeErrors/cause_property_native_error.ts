// test262: test/built-ins/NativeErrors/cause_property_native_error.js
// expect-error: constructor of `RangeError` expects 1 argument(s), got 2
//
// The `{ cause }` options bag (ES2022) is not part of the Error surface in
// spec.md §1.8 — same pin as Error/cause_property.ts, for the subclasses.
// The nativeErrors list is narrowed to the classes that exist here and
// unrolled; the verifyProperty descriptor checks are dropped.

function main(): void {
  const message = "my-message";
  const cause = new Error("my-cause");
  assertSameValue(new RangeError(message, { cause }).cause, cause, "RangeError cause");
  assertSameValue(new SyntaxError(message, { cause }).cause, cause, "SyntaxError cause");
  assertSameValue(new TypeError(message, { cause }).cause, cause, "TypeError cause");
}
