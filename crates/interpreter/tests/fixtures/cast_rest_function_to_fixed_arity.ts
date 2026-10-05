// A function with a rest parameter keeps an exact arity (spec §1.4), so a cast
// from `unknown` to a fixed-arity function type rejects it, even where its Wasm
// arity matches; JavaScript would call it. A cast to a rest type, and casts of
// functions with defaults or captures, still succeed.
function count(...xs: number[]): number {
  return xs.length;
}

function withDefault(a: number, b: number = 10): number {
  return a + b;
}

function rejects(cast: () => void): boolean {
  try {
    cast();
  } catch (e: TypeError) {
    return e.message.includes("type mismatch");
  }
  return false;
}

class Counter {
  base: number = 1;
  add(x: number): number {
    return this.base + x;
  }
}

function main(): void {
  const rest: unknown = count;
  assert(rejects(() => { const f = rest as (x: number) => number; }), "rest to one parameter");
  assert(rejects(() => { const f = rest as (x: number, y: number) => number; }), "rest to two");
  assert((rest as (...xs: number[]) => number)(1, 2, 3) === 3, "rest to rest");

  const arrow: unknown = (...xs: string[]): number => xs.length;
  assert(rejects(() => { const f = arrow as (s: string) => number; }), "rest arrow");

  const defaulted: unknown = withDefault;
  assert((defaulted as (a: number, b: number) => number)(1, 2) === 3, "default supplied");
  assert((defaulted as (a: number) => number)(1) === 11, "default omitted");

  const counter = new Counter();
  const capturing: unknown = (x: number): number => counter.add(x);
  assert((capturing as (x: number) => number)(2) === 3, "capturing closure");
}
