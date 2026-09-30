// A bare `return` is a `void` return, so an unannotated arrow or function
// expression that also returns a value has two return types that do not unify.
// expect-error: return type `number` conflicts with earlier return `void`; return a value on every path or on none
// expect-error: return type `number` conflicts with earlier return `void`
// expect-error: return type `number` conflicts with earlier return `void`
// expect-error: return type `number` conflicts with earlier return `void`
// expect-error: return type `null` conflicts with earlier return `void`
// expect-error-count: 5
function one(): number {
  return 1;
}

function main(): void {
  const key: string = ["a"][0];
  const arrow = () => {
    if (key === "a") {
      return;
    }
    return one();
  };
  const expression = function () {
    if (key === "a") {
      return;
    }
    return 1;
  };
  const mapped = [1, 2].map((x) => {
    if (x === 1) {
      return;
    }
    return x;
  });
  // A contextual `void` signature does not make the value return fit.
  const underVoid: () => void = () => {
    if (key === "a") {
      return;
    }
    return 1;
  };
  const nullable = () => {
    if (key === "a") {
      return;
    }
    return null;
  };
}
