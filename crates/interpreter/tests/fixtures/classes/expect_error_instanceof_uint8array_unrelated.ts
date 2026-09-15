// expect-error: is always false: the types are not related
// Widening the `instanceof` right-hand side to accept `Uint8Array` does not
// widen the relatedness gate: a statically-`string` operand can never be bytes,
// so `unknown` stays the way to write a dynamic test.
function main(): boolean {
  const s: string = "bytes";
  return s instanceof Uint8Array;
}
