import { inc, Doubler } from "@test/lib";

export function midInc(): (a: number) => number {
  return inc;
}

export function midTwice(): (a: number) => number {
  return Doubler.twice;
}
