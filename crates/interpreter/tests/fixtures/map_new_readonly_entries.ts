// SUB-1756: `new Map` takes a readonly array of readonly entry tuples, the type
// a constant entry table has in TypeScript.
const TABLE: readonly (readonly [string, number])[] = [["a", 1], ["b", 2]];
function main(): void {
  const m = new Map<string, number>(TABLE);
  assert(m.size === 2 && m.get("b") === 2);
  const mutable: [string, number][] = [["c", 3]];
  assert(new Map<string, number>(mutable).get("c") === 3);
}
