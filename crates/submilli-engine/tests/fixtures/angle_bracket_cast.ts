// `<T>expr` is TypeScript's original cast syntax and lowers to the same node as
// `expr as T` — same checking, same runtime behavior.
function main(): void {
  const n = <number>1;
  const s = <string>"hi";
  const neg = <number>-5;
  const xs = <number[]>[1, 2];
  const viaAs = 1 as number;

  assert(n === 1, "number");
  assert(s === "hi", "string");
  assert(neg === -5, "binds the unary operand");
  assert(xs.length === 2, "array");
  assert(n === viaAs, "same as the `as` spelling");
}
