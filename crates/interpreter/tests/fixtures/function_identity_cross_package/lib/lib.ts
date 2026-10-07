export function inc(a: number): number {
  return a + 1;
}

export function getInc(): (a: number) => number {
  return inc;
}

export function isInc(f: (a: number) => number): boolean {
  return f === inc;
}

export class Doubler {
  static twice(a: number): number {
    return a * 2;
  }
}

export function getTwice(): (a: number) => number {
  return Doubler.twice;
}

function hidden(a: number): number {
  return a * 3;
}

export { hidden as triple };

export function getTriple(): (a: number) => number {
  return hidden;
}

export { dec, getDec } from "./extra";
