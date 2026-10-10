// expect-error: cannot assign to static readonly field `Counter.LIMIT`
class Counter {
  static readonly LIMIT: number = 10;
}

function main(): void {
  Counter.LIMIT++;
}
