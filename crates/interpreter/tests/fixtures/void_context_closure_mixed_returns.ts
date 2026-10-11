// A closure whose context expects it to return `void` may mix `return;` with
// `return value;`, return values of unrelated types, or return a value on one
// path and run off its end on another, as in tsc: the context discards the
// result, so each value is evaluated and dropped.
function run(f: () => void): void {
  f();
}

function main(): void {
  const out: number[] = [];
  [1, 2, 3].forEach((x) => {
    if (x === 2) {
      return;
    }
    return out.push(x);
  });
  assert(out.length === 2, "forEach skips the bare return");

  let total = 0;
  [1, 2, 3].forEach((x) => {
    if (x > 2) {
      return x;
    }
    total = total + x;
  });
  assert(total === 3, "a value on one path, the end on another");

  // Held as returning `unknown`, such a closure still returns its values.
  const kept: () => void = () => {
    if (total > 0) {
      return "kept";
    }
    return;
  };
  const asUnknown: (() => unknown)[] = [kept];
  assert(asUnknown[0]() === "kept", "a value read through `unknown`");

  let seen = "";
  run(() => {
    if (out.length > 5) {
      return 1;
    }
    seen = "ran";
    return "s";
  });
  assert(seen === "ran", "a call argument");

  const g: () => void = () => {
    if (out.length > 5) {
      return;
    }
    return out.pop();
  };
  g();
  assert(out.length === 1, "the dropped value's expression still runs");

  // A `void` call returned beside a value runs, and the closure returns
  // `undefined` in its place.
  let calls = 0;
  const bump = (): void => {
    calls = calls + 1;
  };
  const mixed: () => void = () => {
    if (calls < 5) {
      return bump();
    }
    return calls;
  };
  const viaUnknown: (() => unknown)[] = [mixed];
  const result = viaUnknown[0]();
  assert(calls === 1, "the returned `void` call runs");
  assert(typeof result !== "number", "it returns no value");
}
