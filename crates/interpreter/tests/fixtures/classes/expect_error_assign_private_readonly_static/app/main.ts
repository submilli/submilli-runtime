// expect-error: static member `SECRET` of class `Counter` is private
// expect-error: static member `hidden` of class `Counter` is private
import { Counter } from "@test/counters";

function noop(): number {
  return 0;
}

function main(): void {
  Counter.SECRET = 7;
  Counter.hidden = noop;
}
