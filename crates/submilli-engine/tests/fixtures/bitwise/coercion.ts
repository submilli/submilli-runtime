function complement(value: number | bigint): number | bigint { return ~value; }
function genericComplement<T>(value: T): number { return ~value; }
function main(): void {
  assert(~"3" === -4 && ~true === -2 && ~false === -1, "unary coercion");
  assert(~"not numeric" === -1, "NaN coercion");
  assert(complement(2) === -3 && complement(2n) === -3n, "numeric union");
  const obj = {valueOf: (): number => 7};
  assert(~obj === -8, "object primitive coercion");
  const bigObject = {valueOf: (): bigint => 7n};
  const fromObject: unknown = ~bigObject;
  assert(fromObject === -8n, "object bigint coercion");
  const fromGeneric: unknown = genericComplement(7n);
  assert(fromGeneric === -8n, "generic bigint coercion");
}
