// A bound that names its own parameter is substituted once, not expanded.
function child<T extends { left: T | null; right: T | null }>(node: T, side: "left" | "right"): T | null { return node[side]; }
function first<T extends { a: T[keyof T] }>(value: T): T[keyof T] { return value.a; }
interface Tree { left: Tree | null; right: Tree | null }

function main(): void {
  const leaf: Tree = { left: null, right: null };
  const root: Tree = { left: leaf, right: null };
  assert(child(root, "left") === leaf && child(root, "right") === null, "an F-bounded receiver reads its children");
  assert(first({ a: 1 }) === 1, "a bound reading its own keys");
}
