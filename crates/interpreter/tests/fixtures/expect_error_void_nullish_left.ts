// `void` has no value to test for null. JavaScript takes the right side of
// `f() ?? g()` every time, which a left side that is `void` on only one path
// cannot be compiled to, so every `void` left side is refused.
// expect-error-count: 2
// expect-error: cannot compare `void`: the left side of `??` must be a value
function f(): void {}

function n(): number {
  return 1;
}

function main(): void {
  const k: string = ["a"][0];
  f() ?? f();
  const x = (k === "a" ? f() : n()) ?? 3;
}
