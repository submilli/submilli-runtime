// expect-error: a static field cannot be optional
class Counter {
  static count?: number = 0;
}

function main(): void {}
