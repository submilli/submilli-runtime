// test262: test/built-ins/Error/cause_property.js
// expect-fail: an omitted Error cause is still an own property (SUB-1830)
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/Error/cause_property.js
// verifyProperty becomes own-property and value checks; descriptor flags are
// excluded by the conformance rules. Object-valued SameValue is structural.

function main(): void {
  const message = "my-message";
  const cause = { message: "my-cause" };
  const error = new Error(message, { cause });

  assertSameValue(Object.hasOwn(error, "cause"), true);
  assertSameValue(error.cause, cause);

  assertSameValue(Object.hasOwn(new Error(message), "cause"), false);
  const explicit = new Error(message, { cause: undefined });
  assertSameValue(Object.hasOwn(explicit, "cause"), true);
  assertSameValue(explicit.cause, undefined);
}
