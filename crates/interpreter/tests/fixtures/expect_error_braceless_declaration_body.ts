// A `let` or `const` can't be the whole body of a statement without braces
// (TS1156): the binding would go out of scope as soon as it was made.
// expect-error: a `let` declaration can't be the body of a statement without braces
// expect-error: a `const` declaration can't be the body of a statement without braces
function main(): void {
  const c: boolean = true;
  if (c) let x: number = 1;
  for (const v of [1]) const y: number = v;
}
