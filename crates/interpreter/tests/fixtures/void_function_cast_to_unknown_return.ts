// A `void` function fits a function type returning `unknown`, as in TypeScript,
// also through a checked cast.
let calls = 0;

function count(): void {
  calls = calls + 1;
}

interface Holder {
  f: () => unknown;
}

function main(): void {
  const holder: Holder = { f: count };
  const erased: unknown = holder;
  const back = erased as Holder;
  back.f();
  const fn: unknown = count;
  const called = fn as () => unknown;
  called();
  assert(calls === 2, "both casts pass");
}
