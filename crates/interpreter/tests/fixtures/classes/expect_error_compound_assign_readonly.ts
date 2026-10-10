// expect-error: cannot assign to readonly field
// expect-error: cannot assign to read-only accessor
// expect-error: postfix `++` is not supported on accessor property
// expect-error: `extra` on `Box` is optional; `+=` requires a field that is never `undefined`
// expect-error: `slack` on `Box` is nullable; `+=` requires a field that is never `null`
// expect-error: write the assignment out: `if (b.slack !== null) { b.slack = b.slack + …; }`
// expect-error: cannot assign to static readonly field
class Box {
  readonly value: number = 1;
  static readonly origin: number = 0;
  extra?: number;
  slack: number | null = null;

  get doubled(): number {
    return this.value * 2;
  }
}

function main(): void {
  const b = new Box();
  b.value += 1;
  b.doubled += 1;
  b.doubled++;
  b.extra += 1;
  // Narrowing does not reach a compound-assign target, so the guard does not
  // make this legal — the written-out `b.slack = b.slack + 1` would be.
  if (b.slack !== null) {
    b.slack += 1;
  }
  Box.origin += 1;
}
