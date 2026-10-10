// expect-error: in TypeScript this is always `true`; run `f();` and `g();` each as its own statement, then use `true`
// expect-error: this comparison compares two nullish values
// expect-error: in TypeScript this is always `false`; write `false`
// expect-error-count: 3
// Both sides' effects are kept; a comparison over several lines is named
// without quoting it; `void x` of a plain reference has no effect to keep.
function f(): void {}
function g(): void {}
function main(): void {
  const x = 1;
  if (void f() == void g()) {}
  if (void f(
  ) == null) {}
  if (void x != undefined) {}
}
