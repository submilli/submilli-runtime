// expect-error: `origin` is a static member of `Counter`
class Counter {
  static origin(): number {
    return 0;
  }
}

function main(): void {
  const c = new Counter();
  c.origin();
}
