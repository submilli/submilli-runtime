import { Counter } from "@test/counters";
import { Ticker } from "@test/tickers";

function main(): void {
  const c = new Counter(1);
  c.count += 2;
  c.count++;
  const old = c.count++;
  assert(old === 4);
  assert(c.count === 5);
  c.total += 7n;
  assert(c.total === 7n);
  c.label += "x";
  assert(c.label === "x");
  assert(c.bump() === 1);
  assert(c.bump() === 2);

  const t = new Ticker();
  t.tick();
  assert(t.count === 15);
  assert(t.ticks === 1);

  t.count += 1;
  const before = t.ticks++;
  assert(before === 1);
  assert(t.ticks === 2);
  assert(t.count === 16);

  // The inherited field reads the same slot through a base-typed reference.
  const base: Counter = t;
  assert(base.count === 16);
}
