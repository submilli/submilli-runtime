import { Maybe, maybe } from "@test/maybe";

function main(): void {
  assert(new Maybe(0).get() === null);
  assert(new Maybe(2).get() === 2);
  assert(new Maybe(-1).getOrNothing() === null);
  assert(new Maybe(4).getOrNothing() === 4);
  assert(maybe(-1) === null);
  assert(maybe(0) === null);
  assert(maybe(3) === 3);
}
