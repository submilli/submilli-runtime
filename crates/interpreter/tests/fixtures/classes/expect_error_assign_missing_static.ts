// expect-error: class `Counter` has no static member `missing`
class Counter {
  static count: number = 0;
}

function main(): void {
  Counter.missing = 1;
}
