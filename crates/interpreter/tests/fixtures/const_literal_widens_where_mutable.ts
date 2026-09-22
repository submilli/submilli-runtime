// A literal type is kept only where the value cannot change. Every position below is
// mutable or infers its type from its contents, so each widens to the base primitive —
// matching TypeScript. Without widening each line is a type error.
function pick(): number { return 2; }

function main(): void {
  const a = 1;
  const b = 2;

  // a `let` is reassignable
  let n = a;
  n = 5;

  // array elements are mutable
  const xs = [a, b];
  xs.push(3);

  // object-literal properties are mutable
  const o = { k: a };
  o.k = 5;

  // equality needs overlap, not assignability, and must not depend on operand order
  const v = pick();
  const eq1 = a === v;
  const eq2 = v === a;

  assert(n === 5, "let widened");
  assert(xs.length === 3, "array widened");
  assert(o.k === 5, "object field widened");
  assert(eq1 === false && eq2 === false, "equality both orders");
}
