// expect-error: `Counter` is a class, not a value
class Counter {
  static origin(): number {
    return 0;
  }
}

function main(): void {
  const x = Counter;
}
