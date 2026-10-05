// An assignment expression has the value it copies, so testing its
// truthiness narrows the copied reference as well as the target, as in
// TypeScript: in `if ((z = x))`, both `z` and `x` are truthy.
type Node = { v: number };

function copied(x: string | null): string {
  let z: string | null = null;
  if ((z = x)) {
    return x + z;
  }
  return "none";
}

function chained(x: number | null): number {
  let y: number | null = null;
  let z: number | null = null;
  if ((z = y = x)) {
    return x + y + z;
  }
  return 0;
}

function field(o: { next: Node | null }): number {
  let n: Node | null = null;
  if ((n = o.next)) {
    return n.v + o.next.v;
  }
  return -1;
}

function main(): void {
  console.log(copied("a"), copied(null));
  console.log(chained(2), chained(null), chained(0));
  console.log(field({ next: { v: 1 } }), field({ next: null }));
}
