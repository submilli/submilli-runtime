// expect-error: `value` is an instance member of `Counter`, not a static
class Counter {
  value(): number {
    return 1;
  }
}

function main(): void {
  Counter.value();
}
