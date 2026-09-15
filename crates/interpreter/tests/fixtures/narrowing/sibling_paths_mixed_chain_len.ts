// Chain lengths 0, 1 and 2 in one guard: the wrap order must nest every path
// inside the prefixes its source reads, and the equal-length siblings must not
// depend on which order the narrow env was walked in.
interface Leaf {
  w: string | null;
  b: string | null;
}

interface Node {
  zed: Leaf | null;
  alpha: string | null;
}

function walk(n: Node | null): string {
  if (n !== null && n.zed !== null && n.zed.w !== null && n.zed.b !== null && n.alpha !== null) {
    return n.alpha + n.zed.w + n.zed.b + n.zed.w.length.toString();
  }
  return "-";
}

function main(): void {
  assert(walk({ zed: { w: "w", b: "b" }, alpha: "a" }) === "awb1", "mixed chain lengths");
  assert(walk(null) === "-", "root null");
  assert(walk({ zed: null, alpha: "a" }) === "-", "zed null");
  assert(walk({ zed: { w: null, b: "b" }, alpha: "a" }) === "-", "w null");
  assert(walk({ zed: { w: "w", b: null }, alpha: "a" }) === "-", "b null");
  assert(walk({ zed: { w: "w", b: "b" }, alpha: null }) === "-", "alpha null");
}
