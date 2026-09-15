// expect-error: class method `toString` must have signature `(): string`
// expect-error: class method `toJson` must have signature `(): string`
class Bad {
  x: number;

  constructor(x: number) {
    this.x = x;
  }

  toString(pad: number): string {
    return "x".repeat(pad);
  }

  toJson(): number {
    return this.x;
  }
}

function main(): void {
  const b: Bad = new Bad(1);
  assert(b.x === 1);
}
