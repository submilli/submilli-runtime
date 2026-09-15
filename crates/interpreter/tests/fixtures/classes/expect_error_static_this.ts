// expect-error: `this` is not available in a static member
class Counter {
  private n: number;
  constructor() {
    this.n = 0;
  }
  static bad(): number {
    return this.n;
  }
}

function main(): void {}
