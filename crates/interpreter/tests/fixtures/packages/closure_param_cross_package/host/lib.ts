export type Reducer = (acc: number, n: number) => number;

export interface Sink {
  emit: (msg: string) => void;
}

/** Invokes a caller-supplied closure inside this package. */
export function invoke(fn: (n: number) => number): number {
  return fn(1);
}

/** Zero-arity, void-returning: the other end of the ABI's result slot. */
export function repeat(times: number, body: () => void): void {
  for (let i = 0; i < times; i = i + 1) {
    body();
  }
}

/** Folds with a closure named only through a package-local alias. */
export function fold(xs: number[], seed: number, r: Reducer): number {
  let acc = seed;
  for (let i = 0; i < xs.length; i = i + 1) {
    acc = r(acc, xs[i]);
  }
  return acc;
}

/** The closure arrives as an interface property rather than a bare parameter. */
export function announce(sink: Sink, msg: string): void {
  sink.emit(msg);
}

/** Returned rather than taken — the closure is built here, run there. */
export function adder(by: number): (n: number) => number {
  return (n: number): number => n + by;
}
