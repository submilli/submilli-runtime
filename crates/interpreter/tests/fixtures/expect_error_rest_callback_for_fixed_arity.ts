// A function with a rest parameter can't stand for a fixed-arity function an
// argument of which doesn't fit its rest element, as in tsc. Submilli also
// rejects one with as many parameters as the fixed-arity function, which tsc
// accepts: an erased cast could not tell its rest arguments need packing.
// expect-error: expected `(arg0: number, arg1: number, arg2: number[]) => U`, got `(...arg0: number[]) => number`
// expect-error: expected `(arg0: number, arg1: number) => number`, got `(arg0: number, ...arg1: number[]) => number`
// expect-error: expected `(arg0: string) => number`, got `(...arg0: number[]) => number`
// expect-error-count: 3
function applyTwo(f: (a: number, b: number) => number): number {
  return f(3, 4);
}

function main(): void {
  const xs = [1, 2];
  console.log(xs.map((...args: number[]) => args.length).join(","));
  console.log(applyTwo((first: number, ...rest: number[]) => first + rest.length));
  const f: (s: string) => number = (...ns: number[]) => ns.length;
  console.log(f("a"));
}
