// The reverse order of `expect_error_bare_return_then_value_return.ts`: the
// bare `return` is the one that conflicts. A `never` return in front does not
// hide it, since it names no type of its own.
// expect-error: return type `void` conflicts with earlier return `number`; return a value on every path or on none
// expect-error: return type `void` conflicts with earlier return `number`
// expect-error: return type `number` conflicts with earlier return `void`; return a value on every path or on none
// expect-error-count: 3
function boom(): never {
  throw new Error("boom");
}

function main(): void {
  const key: string = ["a"][0];
  const arrow = () => {
    if (key === "a") {
      return 1;
    }
    return;
  };
  const expression = function () {
    if (key === "a") {
      return 1;
    }
    return;
  };
  const afterNever = () => {
    if (key === "c") {
      return boom();
    }
    if (key === "b") {
      return;
    }
    return 1;
  };
}
