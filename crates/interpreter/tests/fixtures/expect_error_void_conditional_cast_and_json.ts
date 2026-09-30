// The two value positions with their own hand-written `void` screen: both must
// refuse a conditional that is `void` on one path as they refuse bare `f()`.
// expect-error: cannot cast `void`: it has no value
// expect-error: `JSON.stringify(x)` requires a non-`void` argument
function f(): void {}

function maybe(): number | null {
  return null;
}

function main(): void {
  const c = true;
  const cast = (c ? f() : 1) as number;
  const json = JSON.stringify(maybe() ?? f());
}
