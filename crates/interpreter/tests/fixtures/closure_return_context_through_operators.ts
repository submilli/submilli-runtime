// A function literal takes its return type's context through `??`, through
// an `as` cast, and from the call that invokes it on the spot, as in tsc.
// Under an `unknown` context its returns need not agree, and returns that
// share no one type each fit a context that covers them all.
function main(): void {
  const flag = Math.random() > 2;
  const absent: (() => unknown) | null = null;
  const a: () => unknown = absent ?? (() => {
    if (flag) {
      return 1;
    }
    return "x";
  });
  assert(a() === "x", "the right side of ??");

  const b = (() => {
    if (flag) {
      return 1;
    }
    return "x";
  }) as () => unknown;
  assert(b() === "x", "an as cast");

  const double = ((x) => x * 2) as (x: number) => number;
  assert(double(3) === 6, "a cast types an unannotated parameter");

  const c: unknown = (() => {
    return;
  })();
  assert(c === null, "an immediately invoked closure");

  const d: number | null = (() => {
    if (flag) {
      return null;
    }
    return 4;
  })();
  assert(d === 4, "returns joined by the call's context");

  const e: () => number | null = () => {
    if (flag) {
      return null;
    }
    return 5;
  };
  assert(e() === 5, "returns joined by an annotation's context");
}
