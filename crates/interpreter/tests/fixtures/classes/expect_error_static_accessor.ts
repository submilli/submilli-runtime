// expect-error: static accessors are not supported
class Counter {
  static get limit(): number {
    return 10;
  }
}

function main(): void {}
