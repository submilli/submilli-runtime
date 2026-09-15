// expect-error: cannot assign to readonly field
class Box {
  readonly value: number;
  constructor(v: number) {
    this.value = v;
  }
  reset(): void {
    this.value = 0;
  }
}

function main(): void {}
