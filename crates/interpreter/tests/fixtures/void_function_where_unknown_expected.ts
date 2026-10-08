// A function returning `void` fits where a function returning `unknown` is
// expected, as in tsc: its result is `undefined`, which Submilli spells
// `null`. An unannotated arrow with no `return`, a named `void` function and a
// `void` method all qualify, and a predicate callback that returns nothing is
// falsy.
interface Runner {
  run(): unknown;
}

class Quiet implements Runner {
  run(): void {}
}

let calls = 0;

function noop(): void {
  calls = calls + 1;
}

function takes(f: () => unknown): unknown {
  return f();
}

function each<T>(xs: T[], f: (x: T) => unknown): unknown[] {
  return xs.map(f);
}

function main(): void {
  const g: () => unknown = () => {
    calls = calls + 1;
  };
  const h: () => unknown = noop;
  assert(g() === null, "an annotated variable");
  assert(h() === null, "a named void function");
  assert(takes(() => {}) === null, "a call argument");
  assert(calls === 2, "each function ran");

  const r: Runner = new Quiet();
  assert(r.run() === null, "a void method for an unknown one");
  assert(each([1, 2], (x) => {}).length === 2, "a generic callback");

  const xs = [1, 2, 3];
  assert(xs.filter(() => {}).length === 0, "filter treats no result as falsy");
  assert(!xs.some((x) => {}), "some");
  assert(xs.find(() => {}) === null, "find");

  const fs: Array<() => unknown> = [() => {}, () => 1];
  assert(fs[0]() === null && fs[1]() === 1, "an array of such functions");
}
