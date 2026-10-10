// Literal candidates for a type parameter that is the call's result form
// their union, as in tsc: `pick(1, 2)` is a `1 | 2`, and a `let` copying it
// widens to `number`. A function literal passed for a type parameter keeps
// the literals it returns, as their union when it returns several, also as
// a field of an object literal argument. Two function literal candidates
// widen theirs.
function pick<T>(a: T, b: T): T {
  return b;
}

function three<T>(a: T, b: T, c: T): T {
  return a;
}

function id<T>(x: T): T {
  return x;
}

function run<A>(options: { a: A; b: (a: A) => void }): A {
  return options.a;
}

function main(): void {
  const both: 1 | 2 = pick(1, 2);
  const inferred = pick(1, 2);
  const exact: 1 | 2 = inferred;
  let copy = inferred;
  copy = 7;
  const words: "a" | "b" | "c" = three("a", "b", "c");
  assert(both === 2 && exact === 2 && copy === 7 && words === "a", "literal candidates");

  const flag = 1 as number;
  const branches: () => 1 | 2 = id(() => {
    if (flag > 5) {
      return 1;
    }
    return 2;
  });
  const mixed: () => 1 | "s" = id(() => {
    if (flag > 5) {
      return 1;
    }
    return "s";
  });
  assert(branches() === 2 && mixed() === "s", "a function literal's returns");

  const fromArrow: () => 42 = run({ a: () => 42, b(a) {} });
  const fromFunction: () => 42 = run({
    a: function () {
      return 42;
    },
    b(a) {},
  });
  const fromMethod: () => 42 = run({
    a() {
      return 42;
    },
    b(a) {},
  });
  assert(fromArrow() + fromFunction() + fromMethod() === 126, "a function literal field");

  const later = pick(() => "a", () => "b");
  let other = later();
  other = "z";
  assert(other === "z", "two function literal candidates widen");
}
