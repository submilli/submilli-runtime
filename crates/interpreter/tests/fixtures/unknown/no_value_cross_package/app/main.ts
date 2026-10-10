import { Maybe, maybe } from "@test/maybe";

function main(): void {
  assert(new Maybe(0).get() === undefined);
  assert(new Maybe(2).get() === 2);
  assert(new Maybe(-1).getOrNothing() === undefined);
  assert(new Maybe(4).getOrNothing() === 4);
  assert(maybe(-1) === undefined);
  assert(maybe(0) === undefined);
  assert(maybe(3) === 3);
}
