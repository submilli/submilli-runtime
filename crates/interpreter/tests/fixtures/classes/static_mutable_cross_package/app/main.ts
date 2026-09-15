import { Counter } from "@test/counters";

class Local extends Counter {}

function main(): void {
  Counter.count = 5;
  assert(Counter.read() === 5);

  Counter.count += 2;
  assert(Counter.read() === 7);

  // Through a local subclass name: same definer slot.
  Local.count = 9;
  assert(Counter.read() === 9);

  assert(Counter.LIMIT === 10);
}
