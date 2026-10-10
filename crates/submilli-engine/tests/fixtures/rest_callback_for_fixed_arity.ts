// A function with a rest parameter stands for a fixed-arity function type when
// each argument it would be passed fits, as in tsc: positions past its fixed
// parameters are packed into the rest array. This holds through a parameter,
// a variable, an interface field, a class method, a generic, an array element,
// and an adapter of the adapter, and for a declared function used as a value.
function sum(...ns: number[]): number {
  let total = 0;
  for (const n of ns) total += n;
  return total;
}

function withDefault(a: number, b: number = 100, ...r: number[]): number {
  return a + b + r.length;
}

function applyTwo(f: (a: number, b: number) => number): number {
  return f(3, 4);
}

function applyOne(f: (a: number) => string): string {
  return f(7);
}

function each(f: (a: number, b: number) => void): void {
  f(5, 6);
}

function twice<A>(f: (a: A, b: A) => A, x: A): A {
  return f(x, x);
}

interface Holder {
  cb: (x: string, y: string) => number;
}

class Runner {
  run(f: (a: number, b: number, c: number) => string): string {
    return f(1, 2, 3);
  }
}

function main(): void {
  assert(applyTwo((...ns: number[]) => ns.reduce((s, n) => s + n, 0)) === 7, "every argument is packed");
  assert(applyOne((first: number, ...rest: number[]) => `${first}:${rest.length}`) === "7:0", "nothing past the fixed parameter");
  assert(applyTwo(function (...r: number[]): number { return r.length * 7; }) === 14, "a function expression");
  assert(applyTwo(sum) === 7, "a declared function");
  assert(applyTwo(withDefault) === 7, "a default before the rest parameter is supplied");

  const seen: string[] = [];
  each((...xs: number[]) => {
    seen.push(xs.join(","));
  });
  assert(seen.join(";") === "5,6", "a void callback");

  const g: (a: string, b: string) => number = (...ss: string[]) => ss.length;
  assert(g("x", "y") === 2, "a variable");

  const h: Holder = { cb: (...ss: string[]) => ss.length };
  assert(h.cb("a", "b") === 2, "an interface field");

  assert(new Runner().run((...xs: number[]) => xs.join("+")) === "1+2+3", "a class method");
  assert(twice((...xs: number[]) => xs.length * 10, 4) === 20, "a generic callback");

  const fs: ((a: number, b: number) => number)[] = [(...n: number[]) => n.length, sum];
  assert(fs.map((k) => k(3, 4)).join(",") === "2,7", "array elements");

  const two: (a: number, b: number) => number = sum;
  const three: (a: number, b: number, c: number) => number = two;
  assert(two(1, 2) === 3 && three(5, 6, 7) === 18, "an adapter of the adapter passes every argument");

  const counts: number[] = [];
  [10, 20].forEach((...args: (number | number[])[]) => {
    counts.push(args.length);
  });
  assert(counts.join(",") === "3,3", "a host callback gets every argument");

  const erased: unknown = sum;
  const cast = erased as (a: number, b: number, c: number) => number;
  assert(cast(1, 2, 3) === 6, "a cast from unknown");
}
