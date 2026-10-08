// expect-error: a `function` declaration can't be the body of a statement without braces
// expect-error: a `class` must be declared at the top level of the module
// expect-error: an `interface` declaration can't be the body of a statement without braces
// expect-error: an `enum` declaration can't be the body of a statement without braces
// expect-error: a `type` declaration can't be the body of a statement without braces
// expect-error-count: 8
function main(): void {
  const flag = true;
  if (flag) function f(): void {}
  else function g(): void {}
  while (!flag) function h(): void {}
  do function k(): void {} while (!flag);
  for (const x of [1]) class C {}
  if (flag) interface I { a: number }
  if (flag) enum E { A }
  if (flag) type T = number;
}
