import { Counter } from "@test/counters";

export class Ticker extends Counter {
  ticks: number;

  constructor() {
    super(10);
    this.ticks = 0;
  }

  // `+=` on a field inherited from a class in another package, and `++` on an
  // own field declared after the imported field prefix.
  tick(): void {
    this.count += 5;
    this.ticks++;
  }
}
