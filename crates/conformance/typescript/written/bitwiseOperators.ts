// Written for Submilli: bitwise operand acceptance and inferred results.
// @target: es2020
// @strict: true

function numbers(a: number, b: number): number {
  const and = a & b;
  const or = a | b;
  const xor = a ^ b;
  const not = ~a;
  const left = a << b;
  const right = a >> b;
  const unsigned = a >>> b;
  a &= b; a |= b; a ^= b; a <<= b; a >>= b; a >>>= b;
  return unsigned;
}
function bigints(a: bigint, b: bigint): bigint {
  const and = a & b;
  const or = a | b;
  const xor = a ^ b;
  const not = ~a;
  const left = a << b;
  const right = a >> b;
  a &= b; a |= b; a ^= b; a <<= b; a >>= b;
  return right;
}
function unary(text: string, flag: boolean, value: number | bigint): number | bigint {
  const a = ~text;
  const b = ~flag;
  const c = ~value;
  return c;
}
function errors(a: number, b: bigint, flag: boolean, value: unknown): void {
  const mixed = a & b;
  const unsigned = b >>> b;
  const boolean = flag & flag;
  const unknown = ~value;
}
function main(): void {}
