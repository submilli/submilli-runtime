// `f() ?? x` and `cond ? f() : x` build a union that *lists* `void`. It has no
// more of a runtime value than bare `void`, so every gate that refuses `void`
// has to refuse it too — a top-level-only check lets it reach codegen.
// expect-error: cannot compare `number | void`: a `switch` discriminant must be a value
// expect-error: cannot compare `number | void`: an equality operand must be a value
// expect-error: expected a value in this condition, got `number | void`
function f(): void {}

function main(): void {
  switch (f() ?? 1) {
  }
  const compared = (f() ?? 1) === 1;
  if (true ? f() : 1) {
  }
}
