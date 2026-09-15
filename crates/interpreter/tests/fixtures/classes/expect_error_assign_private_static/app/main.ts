// expect-error: static member `count` of class `Counter` is private
import { Counter } from "@test/counters";

function main(): void {
  Counter.count = 5;
  Counter.count += 1;
  Counter.count++;
}
