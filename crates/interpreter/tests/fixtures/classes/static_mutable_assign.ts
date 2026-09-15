class Counter {
  static count: number = 0;
  static label: string = "n";
  static total: bigint = 0n;
  static readonly LIMIT: number = 10;

  static bump(): void {
    Counter.count += 1;
  }
  static read(): number {
    return Counter.count;
  }
}

function main(): void {
  Counter.count = 5;
  assert(Counter.count === 5);
  Counter.count += 2;
  assert(Counter.count === 7);
  Counter.bump();
  assert(Counter.read() === 8);

  Counter.label += "x";
  assert(Counter.label === "nx");

  Counter.total += 3n;
  assert(Counter.total === 3n);

  assert(Counter.LIMIT === 10);
}
