// expect-error: has no initializer and is not assigned in the constructor
class Conditional {
  value: number;
  constructor(flag: boolean) {
    if (flag) {
      this.value = 1;
    }
    // `value` is unassigned when `flag` is false.
  }
}

function main(): void {
  const c = new Conditional(true);
  assert(c.value === 1);
}
