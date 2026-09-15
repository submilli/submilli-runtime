// expect-error: class `Counter` has no static member `orign`
class Counter {
  static origin(): number {
    return 0;
  }
}

function main(): void {
  Counter.orign();
}
