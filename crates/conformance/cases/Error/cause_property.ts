// test262: test/built-ins/Error/cause_property.js
// expect-error: constructor of `Error` expects 1 argument(s), got 2
//
// The `{ cause }` options bag (ES2022) is not part of the Error surface in
// spec.md §1.8: `new Error(message)` takes exactly the message, and instances
// have no `cause` field. The verifyProperty descriptor checks are dropped
// (blanket rule); the pin keeps the value check, so adding `cause` surfaces here.

function main(): void {
  const message = "my-message";
  const cause = new Error("my-cause");
  const error = new Error(message, { cause });
  assertSameValue(error.cause, cause, "error.cause is the options cause");
}
