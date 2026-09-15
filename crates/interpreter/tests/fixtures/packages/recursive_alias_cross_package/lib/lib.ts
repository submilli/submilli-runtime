// The alias body's recursion back-edge has to survive the package export
// surface: the consumer never writes this shape, so the only route to its Wasm
// subtype is through what this package publishes.
export type Node = { val: number; kids: Node[] };

export function leaf(val: number): Node {
  return { val: val, kids: [] };
}

export function total(n: Node): number {
  let sum: number = n.val;
  for (const k of n.kids) {
    sum = sum + total(k);
  }
  return sum;
}
