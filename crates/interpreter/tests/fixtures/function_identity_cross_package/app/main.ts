import { inc, getInc, isInc, Doubler, getTwice } from "@test/lib";
import { midInc, midTwice } from "@test/mid";

function main(): void {
  // The consumer reads `inc` first, so its closure fills the shared cache.
  assert(inc === getInc(), "the consumer's read and the package's read");
  assert(isInc(inc), "the package compares the consumer's closure");
  assert(new Set([inc, getInc(), inc, midInc()]).size === 1, "one closure in a set");
  assert(midInc() === inc, "a second consumer's read");
  // Here the package reads first.
  assert(getTwice() === Doubler.twice, "a static method read in two packages");
  assert(midTwice() === getTwice(), "a static method read by a second consumer");
  assert(getInc()(1) === 2 && midTwice()(4) === 8, "the shared closures call their functions");
}
