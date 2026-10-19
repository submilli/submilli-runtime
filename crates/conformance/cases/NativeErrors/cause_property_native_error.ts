// test262: test/built-ins/NativeErrors/cause_property_native_error.js
// expect-fail: an omitted native-error cause is still an own property (SUB-1830)
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/NativeErrors/cause_property_native_error.js
// The nativeErrors loop is unrolled because classes are not values; EvalError
// is unavailable. verifyProperty becomes own-property and value checks, omitting
// descriptor flags. Object-valued SameValue is structural.

function main(): void {
  {
    const message = "my-message";
    const cause = { message: "my-cause" };
    const error = new Error(message, { cause });

    assertSameValue(Object.hasOwn(error, "cause"), true);
    assertSameValue(error.cause, cause);

    assertSameValue(Object.hasOwn(new Error(message), "cause"), false);
    const explicit = new Error(message, { cause: undefined });
    assertSameValue(Object.hasOwn(explicit, "cause"), true);
    assertSameValue(explicit.cause, undefined);
    assertSameValue(Object.hasOwn(new Error(message, {}), "cause"), false);
  }

  {
    const message = "my-message";
    const cause = { message: "my-cause" };
    const error = new RangeError(message, { cause });

    assertSameValue(Object.hasOwn(error, "cause"), true);
    assertSameValue(error.cause, cause);

    assertSameValue(Object.hasOwn(new RangeError(message), "cause"), false);
    const explicit = new RangeError(message, { cause: undefined });
    assertSameValue(Object.hasOwn(explicit, "cause"), true);
    assertSameValue(explicit.cause, undefined);
    assertSameValue(Object.hasOwn(new RangeError(message, {}), "cause"), false);
  }

  {
    const message = "my-message";
    const cause = { message: "my-cause" };
    const error = new ReferenceError(message, { cause });

    assertSameValue(Object.hasOwn(error, "cause"), true);
    assertSameValue(error.cause, cause);

    assertSameValue(Object.hasOwn(new ReferenceError(message), "cause"), false);
    const explicit = new ReferenceError(message, { cause: undefined });
    assertSameValue(Object.hasOwn(explicit, "cause"), true);
    assertSameValue(explicit.cause, undefined);
    assertSameValue(Object.hasOwn(new ReferenceError(message, {}), "cause"), false);
  }

  {
    const message = "my-message";
    const cause = { message: "my-cause" };
    const error = new SyntaxError(message, { cause });

    assertSameValue(Object.hasOwn(error, "cause"), true);
    assertSameValue(error.cause, cause);

    assertSameValue(Object.hasOwn(new SyntaxError(message), "cause"), false);
    const explicit = new SyntaxError(message, { cause: undefined });
    assertSameValue(Object.hasOwn(explicit, "cause"), true);
    assertSameValue(explicit.cause, undefined);
    assertSameValue(Object.hasOwn(new SyntaxError(message, {}), "cause"), false);
  }

  {
    const message = "my-message";
    const cause = { message: "my-cause" };
    const error = new TypeError(message, { cause });

    assertSameValue(Object.hasOwn(error, "cause"), true);
    assertSameValue(error.cause, cause);

    assertSameValue(Object.hasOwn(new TypeError(message), "cause"), false);
    const explicit = new TypeError(message, { cause: undefined });
    assertSameValue(Object.hasOwn(explicit, "cause"), true);
    assertSameValue(explicit.cause, undefined);
    assertSameValue(Object.hasOwn(new TypeError(message, {}), "cause"), false);
  }

  {
    const message = "my-message";
    const cause = { message: "my-cause" };
    const error = new URIError(message, { cause });

    assertSameValue(Object.hasOwn(error, "cause"), true);
    assertSameValue(error.cause, cause);

    assertSameValue(Object.hasOwn(new URIError(message), "cause"), false);
    const explicit = new URIError(message, { cause: undefined });
    assertSameValue(Object.hasOwn(explicit, "cause"), true);
    assertSameValue(explicit.cause, undefined);
    assertSameValue(Object.hasOwn(new URIError(message, {}), "cause"), false);
  }
}
