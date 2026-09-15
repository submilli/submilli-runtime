export class Counter {
  count: number;
  total: bigint;
  label: string;
  private hits: number;

  constructor(start: number) {
    this.count = start;
    this.total = 0n;
    this.label = "";
    this.hits = 0;
  }

  // `+=` on a private field, inside the package that declares it.
  bump(): number {
    this.hits += 1;
    return this.hits;
  }
}
