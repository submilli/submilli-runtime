// expect-error: cannot assign to static readonly field `Counter.LIMIT`
import { Counter } from "@test/counters";

function main(): void {
  Counter.LIMIT = 20;
}
