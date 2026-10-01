// expect-error: method `Counter.pick` does not return a value on all paths
// expect-error: getter `Counter.sign` does not return a value on all paths
// expect-error: arrow function does not return a value on all paths
// expect-error-count: 3
class Counter {
  private count: number = 0;
  readonly next: () => number = () => {
    if (this.count > 0) {
      return this.count;
    }
  };

  pick(flag: boolean): number {
    if (flag) {
      return 1;
    }
  }

  get sign(): number {
    if (this.count > 0) {
      return 1;
    }
  }
}

function main(): void {
  console.log(new Counter().pick(true).toString());
}
