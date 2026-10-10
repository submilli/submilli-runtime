// expect-error: cannot assign to static method `Counter.reset`
class Counter {
  static count: number = 0;
  static reset(): void {
    Counter.count = 0;
  }
}

function main(): void {
  Counter.reset = Counter.reset;
}
