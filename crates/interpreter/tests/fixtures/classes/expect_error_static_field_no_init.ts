// expect-error: a static field must be initialized
class Counter {
  static readonly LIMIT: number;
  static count: number;
}

function main(): void {}
