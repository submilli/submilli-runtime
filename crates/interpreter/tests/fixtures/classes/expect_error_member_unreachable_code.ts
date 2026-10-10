// expect-error: unreachable code
// expect-error-count: 3
class Counter {
  private count: number = 0;

  constructor() {
    return;
    this.count = 1;
  }

  read(): number {
    return this.count;
    console.log("after return");
  }

  set value(next: number) {
    throw new Error("read-only");
    this.count = next;
  }
}

function main(): void {
  console.log(new Counter().read().toString());
}
