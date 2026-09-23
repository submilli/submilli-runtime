// An assignment that may not run — on the right of `&&`, `||`, or `??`, or in
// a ternary branch — undoes a narrowing on its target, since it may have run,
// but does not narrow the target itself. TypeScript's type here is
// `number | null` too (TS18047).
// expect-error: `+` not defined for `number | null` and `number`

function f(c: boolean): number {
  let x: number | null = 1;
  if (x !== null) {
    const done = c && (x = null) === null;
    return x + 1;
  }
  return 0;
}

function main(): void {
  console.log(f(true));
}
