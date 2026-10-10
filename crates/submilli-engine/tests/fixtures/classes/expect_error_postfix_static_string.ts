// expect-error: `++` expects
class Counter {
  static label: string = "n";
}

function main(): void {
  Counter.label++;
}
