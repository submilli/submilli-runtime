// expect-error: expected `number`, got `string`
class Counter {
  static count: number = 0;
}

function main(): void {
  Counter.count = "x";
}
