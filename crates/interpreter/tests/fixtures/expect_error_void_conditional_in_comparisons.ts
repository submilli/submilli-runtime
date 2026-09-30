// `cond ? f() : x` and `a ?? f()` may produce no value. Each is `void` as a
// whole, so every gate that refuses `void` refuses it too.
// expect-error: cannot compare `void`: a `switch` discriminant must be a value
// expect-error: cannot compare `void`: an equality operand must be a value
// expect-error: expected a value in this condition, got `void`
function f(): void {}

function maybe(): number | null {
  return null;
}

function main(): void {
  switch (maybe() ?? f()) {
  }
  const compared = (true ? f() : 1) === 1;
  if (true ? f() : 1) {
  }
}
