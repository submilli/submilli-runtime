function identity<T>(value: T): T { return value; }
enum Mask { Read = 1, Write = 2 }
function main(): void {
  const matrix: Array<Array<number>> = [[8]];
  const cloned = identity<Array<Array<number>>>(matrix);
  assert((cloned[0][0] >> 1) === 4, "nested generic and shift");
  const mask = Mask.Read | Mask.Write;
  assert(mask === 3, "numeric enums");
  const comparison = 8 >> 1 > 2;
  assert(comparison, "shift before comparison");
}
