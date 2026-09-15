// The two value positions with their own hand-written `void` screen: both must
// use the union-aware predicate, or `f() ?? 1` walks past a gate that refuses
// bare `f()`.
// expect-error: cannot cast `number | void`: it has no value
// expect-error: `JSON.stringify(x)` requires a non-`void` argument
function f(): void {}

function main(): void {
  const c = true;
  const cast = (c ? f() : 1) as number;
  const json = JSON.stringify(f() ?? 1);
}
